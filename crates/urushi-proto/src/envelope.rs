//! Constructors for [`Envelope`]s, so callers don't have to nest `Some(Kind::…(…))` by hand.

use crate::v1::{
    self, Envelope, ErrorCode, envelope::Kind, event, notification, request, response,
};

/// A request with correlation id `id`.
pub fn request(id: u64, body: request::Body) -> Envelope {
    Envelope {
        kind: Some(Kind::Request(v1::Request {
            id,
            body: Some(body),
        })),
    }
}

/// A response to request `id`.
pub fn response(id: u64, outcome: response::Outcome) -> Envelope {
    Envelope {
        kind: Some(Kind::Response(v1::Response {
            id,
            outcome: Some(outcome),
        })),
    }
}

/// An empty successful response to request `id`.
pub fn ok(id: u64) -> Envelope {
    response(id, response::Outcome::Ok(v1::Empty {}))
}

/// An error response to request `id`.
pub fn error(id: u64, code: ErrorCode, message: impl Into<String>) -> Envelope {
    response(
        id,
        response::Outcome::Error(v1::Error {
            code: code.into(),
            message: message.into(),
        }),
    )
}

/// An unsolicited daemon event.
pub fn event(body: event::Body) -> Envelope {
    Envelope {
        kind: Some(Kind::Event(v1::Event { body: Some(body) })),
    }
}

/// A fire-and-forget client notification.
pub fn notification(body: notification::Body) -> Envelope {
    Envelope {
        kind: Some(Kind::Notification(v1::Notification { body: Some(body) })),
    }
}

/// The `Hello` every client sends first.
pub fn hello(id: u64, client_name: &str, client_version: &str) -> Envelope {
    request(
        id,
        request::Body::Hello(v1::Hello {
            protocol_major: crate::PROTOCOL_MAJOR,
            protocol_minor: crate::PROTOCOL_MINOR,
            client_name: client_name.to_owned(),
            client_version: client_version.to_owned(),
        }),
    )
}
