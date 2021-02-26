use std::collections::hash_map;
use std::collections::HashMap;
use std::sync::Arc;

use crate::server::handler::ServerHandler;
use crate::server::req::ServerRequest;
use crate::solicit::header::Headers;
use crate::ServerResponse;

#[derive(Default)]
struct Node {
    service: Option<Arc<dyn ServerHandler>>,
    children: HashMap<String, Node>,
}

impl Node {
    fn add_service(&mut self, path: &str, service: Arc<dyn ServerHandler>) {
        match split_path(path) {
            None => {
                self.service = Some(service);
            }
            Some((first, rem)) => {
                let node = match self.children.entry(first.to_owned()) {
                    hash_map::Entry::Occupied(e) => e.into_mut(),
                    hash_map::Entry::Vacant(e) => e.insert(Node {
                        service: None,
                        children: HashMap::new(),
                    }),
                };
                node.add_service(rem, service);
            }
        }
    }

    fn remove_service(&mut self, path: &str) -> Option<Arc<dyn ServerHandler>> {
        match split_path(path) {
            None => self.service.take(),
            Some((first, rem)) => match self.children.get_mut(first) {
                Some(child) => child.remove_service(rem),
                None => None,
            },
        }
    }

    fn find_service(&self, path: &str) -> Option<&dyn ServerHandler> {
        if let Some((first, rem)) = split_path(path) {
            if let Some(node) = self.children.get(first) {
                if let Some(service) = node.find_service(rem) {
                    return Some(service);
                }
            }
        }

        self.service.as_ref().map(|a| a.as_ref())
    }
}

fn split_path(mut path: &str) -> Option<(&str, &str)> {
    path = path.trim_start_matches('/');

    if path.is_empty() {
        None
    } else {
        let slash = path.find('/');
        match slash {
            Some(slash) => Some((&path[..slash], &path[slash + 1..])),
            None => Some((path, "")),
        }
    }
}

#[test]
fn test_split_path() {
    assert_eq!(None, split_path(""));
    assert_eq!(None, split_path("/"));
    assert_eq!(Some(("first", "")), split_path("first"));
    assert_eq!(Some(("first", "")), split_path("first/"));
    assert_eq!(Some(("first", "")), split_path("/first"));
    assert_eq!(Some(("first", "")), split_path("/first/"));
    assert_eq!(Some(("first", "second")), split_path("/first/second"));
    assert_eq!(Some(("first", "second/")), split_path("/first/second/"));
    assert_eq!(
        Some(("first", "second/third")),
        split_path("/first/second/third")
    );
    assert_eq!(
        Some(("first", "second/third/")),
        split_path("/first/second/third/")
    );
}

/// Convient implementation of `ServerHandler` which allows delegation to
/// multiple `ServerHandler` implementations provided by user.
#[derive(Default)]
pub struct ServerHandlerPaths {
    root: Node,
}

impl ServerHandlerPaths {
    /// Create a new `Service` implementation which returns `404`
    /// on all requests by default.
    pub fn new() -> ServerHandlerPaths {
        Default::default()
    }

    /// Register a service for given path.
    ///
    /// ```
    /// # use std::sync::Arc;
    /// # use httpbis::*;
    /// # use httpbis;
    ///
    /// struct Root {}
    /// struct Files {}
    ///
    /// impl ServerHandler for Root {
    ///     fn start_request(&self, _context: ServerHandlerContext, _req: ServerRequest, mut resp: ServerResponse) -> httpbis::Result<()> {
    ///         resp.send_found_200_plain_text("This is root page")?;
    ///         Ok(())
    ///     }
    /// }
    ///
    /// impl ServerHandler for Files {
    ///     fn start_request(&self, _context: ServerHandlerContext, _req: ServerRequest, mut resp: ServerResponse) -> httpbis::Result<()> {
    ///         resp.send_found_200_plain_text("This is files")?;
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let mut server = ServerBuilder::new_plain();
    /// server.service.set_service("/", Arc::new(Root{}));
    /// server.service.set_service("/files", Arc::new(Files{}));
    /// ```
    pub fn set_service(&mut self, path: &str, service: Arc<dyn ServerHandler>) {
        assert!(path.starts_with("/"));
        self.root.add_service(path, service);
    }

    pub fn set_service_fn<F>(&mut self, path: &str, service: F)
    where
        F: Fn(ServerRequest, ServerResponse) -> crate::Result<()> + Send + Sync + 'static,
    {
        impl<F: Fn(ServerRequest, ServerResponse) -> crate::Result<()> + Send + Sync + 'static>
            ServerHandler for F
        {
            fn start_request(&self, req: ServerRequest, resp: ServerResponse) -> crate::Result<()> {
                self(req, resp)
            }
        }

        self.set_service(path, Arc::new(service))
    }

    pub fn remove_service(&mut self, path: &str) -> Option<Arc<dyn ServerHandler>> {
        assert!(path.starts_with("/"));
        self.root.remove_service(path)
    }

    fn find_service(&self, path: &str) -> Option<&dyn ServerHandler> {
        self.root.find_service(path)
    }
}

impl ServerHandler for ServerHandlerPaths {
    fn start_request(&self, req: ServerRequest, mut resp: ServerResponse) -> crate::Result<()> {
        if let Some(service) = self.find_service(req.headers.path()) {
            info!("invoking user callback for path {}", req.headers.path());
            service.start_request(req, resp)
        } else {
            info!("serving 404 for path {}", req.headers.path());
            drop(resp.send_headers(Headers::not_found_404()));
            drop(resp.close());
            Ok(())
        }
    }
}
