// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Praxis Contributors

//! Bounded Metering exchanges through the core outbound pipeline executor.

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use praxis_ai_apis::{
    callout_target::AddressPolicy,
    subrequest::{self, SubRequest, SubRequestClient, SubResponse},
};
use praxis_core::connectivity::{UrlResolutionPolicy, prepare_url_target_with_policy};
use praxis_filter::{
    CalloutOutcome, CalloutResponse, FilterError, FilterPipeline, FilterRegistry, FilteredSubrequestExecutor,
    RequestExtensions, StagedUpstream, StagedUpstreamFallback, SubrequestRuntime,
};

/// Entitlement verdicts and event acknowledgements must stay bounded.
const MAX_RESPONSE_BYTES: usize = 65_536; // 64 KiB

/// Shared transport and pipeline for the operator-configured Metering endpoint.
#[derive(Clone)]
pub(crate) struct MeteringCallout {
    /// Address admission also applies to each resolved fallback endpoint.
    address_policy: AddressPolicy,
    /// Shared connection pool.
    client: SubRequestClient,
    /// The existing configuration has no outbound filters. Keep an explicit
    /// empty chain so transport still uses core's filtered callout lifecycle.
    pipeline: Arc<FilterPipeline>,
    /// One deadline covers resolution, exchange, and response handling.
    timeout: Duration,
}

impl MeteringCallout {
    /// Build the shared outbound pipeline once per filter construction.
    pub(crate) fn new(
        client: SubRequestClient,
        address_policy: AddressPolicy,
        timeout: Duration,
    ) -> Result<Self, FilterError> {
        let mut pipeline = FilterPipeline::build(&mut [], &FilterRegistry::with_builtins())?;
        pipeline.set_allow_private_upstreams(matches!(address_policy, AddressPolicy::AllowPrivate));
        Ok(Self {
            address_policy,
            client,
            pipeline: Arc::new(pipeline),
            timeout,
        })
    }

    /// Resolve an operator URL and execute a buffered, bounded exchange.
    pub(crate) async fn execute(&self, url: &str, request: SubRequest) -> Result<SubResponse, FilterError> {
        let started = Instant::now();
        let deadline = started + self.timeout;
        let target = prepare_url_target_with_policy(
            url,
            deadline,
            subrequest::address_policy_hook(self.address_policy),
            UrlResolutionPolicy::OperatorCached,
        )
        .await
        .map_err(|error| subrequest::map_url_error(subrequest::UrlSubRequestError::Target(error)))?;
        let mut extensions = RequestExtensions::default();
        extensions.insert(StagedUpstream::from_prepared_target(&target)?);
        extensions.insert(StagedUpstreamFallback::from_prepared_target(&target));
        let prepared = target.bind(request);
        let executor = FilteredSubrequestExecutor::for_callout(
            self.client.clone(),
            SubrequestRuntime::new(None, false, None, started),
            0,
            MAX_RESPONSE_BYTES,
            self.timeout,
        );
        match execute_filtered(&executor, &self.pipeline, prepared.request(), extensions, deadline).await? {
            CalloutOutcome::Response(CalloutResponse::Buffered(response)) => Ok(response),
            CalloutOutcome::ResponseTooLarge { .. } => Err("enmaas_policy: Metering response exceeded 64 KiB".into()),
            _ => Err("enmaas_policy: Metering callout requires a buffered response".into()),
        }
    }
}

/// Allocate the core exchange state separately from URL resolution and staging
/// so their temporary state does not share one large stack frame.
fn execute_filtered<'a>(
    executor: &'a FilteredSubrequestExecutor,
    pipeline: &'a Arc<FilterPipeline>,
    request: &'a SubRequest,
    extensions: RequestExtensions,
    deadline: Instant,
) -> Pin<Box<impl Future<Output = Result<CalloutOutcome, FilterError>> + Send + 'a>> {
    Box::pin(executor.run_classified(pipeline, request, extensions, deadline))
}
