//! Request/response plumbing between communications sources and the [`Engine`].
//!
//! Sources (USB, TCP, UART, ...) share a single [`RequestChannel`] into the engine, tagging
//! each [`Request`] with the [`Address`] it originated from. The engine service holds a
//! [`Router`] containing a response channel for every supported transport, and returns each
//! [`Response`] to the transport named by that address.

// TODO: drop once the transports exercise the whole port API
#![allow(dead_code)]

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender, TryReceiveError, TrySendError};

use red_proto::{req::Req, resp::Resp};

/// Number of requests that may be queued to the engine across all sources.
pub const REQUEST_QUEUE_LEN: usize = 8;

/// Number of responses that may be queued back to a single source.
pub const RESPONSE_QUEUE_LEN: usize = 4;

/// Channel on which the engine receives requests from all sources.
pub type RequestChannel = Channel<CriticalSectionRawMutex, Request, REQUEST_QUEUE_LEN>;

/// Sender half of the [`RequestChannel`], held by each source.
pub type RequestSender = Sender<'static, CriticalSectionRawMutex, Request, REQUEST_QUEUE_LEN>;

/// Receiver half of the [`RequestChannel`], held by the engine.
pub type RequestReceiver = Receiver<'static, CriticalSectionRawMutex, Request, REQUEST_QUEUE_LEN>;

/// Channel on which a single transport receives its responses.
pub type ResponseChannel = Channel<CriticalSectionRawMutex, Response, RESPONSE_QUEUE_LEN>;

/// Sender half of a [`ResponseChannel`], held by the engine via the [`Router`].
pub type ResponseSender = Sender<'static, CriticalSectionRawMutex, Response, RESPONSE_QUEUE_LEN>;

/// Receiver half of a [`ResponseChannel`], held by the transport.
pub type ResponseReceiver =
    Receiver<'static, CriticalSectionRawMutex, Response, RESPONSE_QUEUE_LEN>;

/// The communications source a request arrived on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum Source {
    /// USB device interface
    Usb,
    /// UDP socket
    Udp,
    /// Serial port
    Uart,
    /// Generated on-device (self test, scheduled measurement, ...)
    Internal,
}

impl Source {
    /// Number of supported sources.
    pub const COUNT: usize = 4;

    /// Index of the source within a [`Router`].
    pub const fn index(&self) -> usize {
        match self {
            Source::Usb => 0,
            Source::Udp => 1,
            Source::Uart => 2,
            Source::Internal => 3,
        }
    }
}

/// The return address for a request: the source it arrived on, plus a source specific
/// endpoint used to demultiplex where a source handles more than one peer (socket index,
/// USB interface, etc.). Sources with a single peer should use [`Address::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub struct Address {
    /// Source the request arrived on
    pub source: Source,
    /// Source specific endpoint identifier
    pub endpoint: u16,
}

impl Address {
    /// Creates an address for a source with a single endpoint.
    pub const fn new(source: Source) -> Self {
        Self {
            source,
            endpoint: 0,
        }
    }

    /// Creates an address for a specific endpoint within a source.
    pub const fn endpoint(source: Source, endpoint: u16) -> Self {
        Self { source, endpoint }
    }
}

/// A request for the engine, tagged with the address it originated from.
#[derive(Debug, Clone, defmt::Format)]
pub struct Request {
    /// The address the request arrived from, and to which the response is returned
    pub address: Address,
    /// The request to be handled
    pub req: Req,
}

/// A response from the engine, tagged with the address it is destined for.
#[derive(Debug, Clone, defmt::Format)]
pub struct Response {
    /// The address the originating request arrived from
    pub address: Address,
    /// The response to be returned
    pub resp: Resp,
}

/// Reasons a response could not be delivered to its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum RouteError {
    /// No response channel is registered for the source
    Unsupported(Source),
    /// The source's response channel is full
    Full,
}

/// Response channels for each supported transport, held by the engine service.
///
/// Sources not registered here are unroutable; responses addressed to them fail with
/// [`RouteError::Unsupported`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Router {
    routes: [Option<ResponseSender>; Source::COUNT],
}

impl Router {
    /// Creates a router with no routes registered.
    pub const fn new() -> Self {
        Self {
            routes: [None; Source::COUNT],
        }
    }

    /// Registers the response channel for a source, replacing any existing route.
    pub fn register(&mut self, source: Source, responses: ResponseSender) -> &mut Self {
        self.routes[source.index()] = Some(responses);
        self
    }

    /// Fetches the response channel for a source, if one is registered.
    pub fn route(&self, source: Source) -> Option<ResponseSender> {
        self.routes[source.index()]
    }

    /// Returns a response to its source, failing if the source is unsupported or its
    /// queue is full.
    pub fn try_send(&self, resp: Response) -> Result<(), RouteError> {
        let source = resp.address.source;

        let route = self.route(source).ok_or(RouteError::Unsupported(source))?;

        route.try_send(resp).map_err(|_| RouteError::Full)
    }

    /// Returns a response to its source, waiting for queue space if required.
    ///
    /// Note that this blocks the caller while the source's queue is full, stalling the
    /// engine on behalf of every other source; prefer [`Router::try_send`] in the
    /// engine's dispatch loop.
    pub async fn send(&self, resp: Response) -> Result<(), RouteError> {
        let source = resp.address.source;

        let route = self.route(source).ok_or(RouteError::Unsupported(source))?;

        route.send(resp).await;

        Ok(())
    }
}

/// A source's attachment to the engine: submits requests on the shared [`RequestChannel`]
/// and receives the matching responses on its own [`ResponseChannel`].
///
/// Cheap to copy, so one port may be shared between the tasks servicing a source.
#[derive(Debug, Clone, Copy)]
pub struct Port {
    source: Source,
    requests: RequestSender,
    responses: ResponseReceiver,
}

impl Port {
    /// Attaches a source to the engine via the request channel and its own response channel.
    ///
    /// The matching [`ResponseSender`] must be registered with the engine's [`Router`].
    pub const fn new(source: Source, requests: RequestSender, responses: ResponseReceiver) -> Self {
        Self {
            source,
            requests,
            responses,
        }
    }

    /// The source this port serves.
    pub fn source(&self) -> Source {
        self.source
    }

    /// Submits a request from the given endpoint, waiting for queue space if required.
    pub async fn request(&self, endpoint: u16, req: Req) {
        self.requests.send(self.build(endpoint, req)).await
    }

    /// Submits a request from the given endpoint, failing if the engine queue is full.
    pub fn try_request(&self, endpoint: u16, req: Req) -> Result<(), TrySendError<Request>> {
        self.requests.try_send(self.build(endpoint, req))
    }

    /// Awaits the next response for this source.
    pub async fn receive(&self) -> Response {
        self.responses.receive().await
    }

    /// Fetches the next response for this source if one is pending.
    pub fn try_receive(&self) -> Result<Response, TryReceiveError> {
        self.responses.try_receive()
    }

    /// Builds a request addressed to this port.
    fn build(&self, endpoint: u16, req: Req) -> Request {
        Request {
            address: Address::endpoint(self.source, endpoint),
            req,
        }
    }
}
