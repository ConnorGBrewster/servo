/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#[cfg(feature = "servo")] use heapsize::HeapSizeOf;
use ServoUrl;
use servo_rand::{self, Rng};
use std::sync::Arc;
use url::Host;
use url::idna::domain_to_unicode;
use uuid::Uuid;

// Should we have rust-url publicly export this function?
fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        "ftp" => Some(21),
        "gopher" => Some(70),
        _ => None,
    }
}

pub fn url_origin(url: &ServoUrl) -> Origin {
    let scheme = url.scheme();
    match scheme {
        "blob" => {
            let result = ServoUrl::parse(url.path());
            match result {
                Ok(ref url) => url_origin(url),
                Err(_)  => Origin::new_opaque()
            }
        },
        "ftp" | "gopher" | "http" | "https" | "ws" | "wss" => {
            Origin::Tuple(scheme.to_owned(), url.host().unwrap().to_owned(),
                url.port_or_known_default().unwrap())
        },
        // TODO: Figure out what to do if the scheme is a file
        "file" => Origin::new_opaque(),
        _ => Origin::new_opaque()
    }
}

/// The origin of an URL
#[derive(PartialEq, Eq, Clone, Debug)]
#[cfg_attr(feature = "servo", derive(Deserialize, Serialize))]
pub enum Origin {
    /// A globally unique identifier
    Opaque(OpaqueOrigin),

    /// Consists of the URL's scheme, host and port
    Tuple(String, Host<String>, u16)
}

#[cfg(feature = "servo")]
impl HeapSizeOf for Origin {
    fn heap_size_of_children(&self) -> usize {
        match *self {
            Origin::Tuple(ref scheme, ref host, _) => {
                scheme.heap_size_of_children() +
                host.heap_size_of_children()
            },
            _ => 0,
        }
    }
}

impl Origin {
    /// Creates a new opaque origin that is only equal to itself.
    pub fn new_opaque() -> Origin {
        Origin::Opaque(OpaqueOrigin(servo_rand::thread_rng().gen()))
    }

    /// Return whether this origin is a (scheme, host, port) tuple
    /// (as opposed to an opaque origin).
    pub fn is_tuple(&self) -> bool {
        match *self {
            Origin::Opaque(..) => false,
            Origin::Tuple(..) => true,
        }
    }

    /// https://html.spec.whatwg.org/multipage/#ascii-serialisation-of-an-origin
    pub fn ascii_serialization(&self) -> String {
        match *self {
            Origin::Opaque(_) => "null".to_owned(),
            Origin::Tuple(ref scheme, ref host, port) => {
                if default_port(scheme) == Some(port) {
                    format!("{}://{}", scheme, host)
                } else {
                    format!("{}://{}:{}", scheme, host, port)
                }
            }
        }
    }

    /// https://html.spec.whatwg.org/multipage/#unicode-serialisation-of-an-origin
    pub fn unicode_serialization(&self) -> String {
        match *self {
            Origin::Opaque(_) => "null".to_owned(),
            Origin::Tuple(ref scheme, ref host, port) => {
                let host = match *host {
                    Host::Domain(ref domain) => {
                        let (domain, _errors) = domain_to_unicode(domain);
                        Host::Domain(domain)
                    }
                    _ => host.clone()
                };
                if default_port(scheme) == Some(port) {
                    format!("{}://{}", scheme, host)
                } else {
                    format!("{}://{}:{}", scheme, host, port)
                }
            }
        }
    }
}

/// Opaque identifier for URLs that have file or other schemes
#[derive(Eq, PartialEq, Clone, Debug)]
#[cfg_attr(feature = "servo", derive(Deserialize, Serialize))]
pub struct OpaqueOrigin(Uuid);

#[cfg(feature = "servo")]
known_heap_size!(0, OpaqueOrigin);

/// A representation of an [origin](https://html.spec.whatwg.org/multipage/#origin-2).
#[derive(PartialEq, Eq, Clone, Debug)]
#[cfg_attr(feature = "servo", derive(HeapSizeOf, Deserialize, Serialize))]
pub struct ServoOrigin(Arc<Origin>);

impl ServoOrigin {
    /// Create a new origin comprising a unique, opaque identifier.
    pub fn opaque_identifier() -> ServoOrigin {
        ServoOrigin(Arc::new(Origin::new_opaque()))
    }

    /// Create a new origin for the given URL.
    pub fn new(url: &ServoUrl) -> ServoOrigin {
        ServoOrigin(Arc::new(url.origin()))
    }

    /// Does this origin represent a host/scheme/port tuple?
    pub fn is_scheme_host_port_tuple(&self) -> bool {
        self.0.is_tuple()
    }

    /// Return the host associated with this origin.
    pub fn host(&self) -> Option<&Host<String>> {
        match *self.0 {
            Origin::Tuple(_, ref host, _) => Some(host),
            Origin::Opaque(..) => None,
        }
    }

    /// https://html.spec.whatwg.org/multipage/#same-origin
    pub fn same_origin(&self, other: &ServoOrigin) -> bool {
        self.0 == other.0
    }

    pub fn copy(&self) -> ServoOrigin {
        ServoOrigin(Arc::new((*self.0).clone()))
    }

    pub fn alias(&self) -> ServoOrigin {
        ServoOrigin(self.0.clone())
    }
}
