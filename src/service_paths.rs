use std::collections::hash_map;
use std::collections::HashMap;
use std::sync::Arc;

use data_or_trailers::HttpStreamAfterHeaders;
use result;
use service::Service;
use service::ServiceContext;
use solicit::header::Headers;
use ServerSender;

#[derive(Default)]
struct Node {
    service: Option<Arc<Service>>,
    children: HashMap<String, Node>,
}

impl Node {
    fn add_service(&mut self, path: &str, service: Arc<Service>) {
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

    fn remove_service(&mut self, path: &str) -> Option<Arc<Service>> {
        match split_path(path) {
            None => self.service.take(),
            Some((first, rem)) => match self.children.get_mut(first) {
                Some(child) => child.remove_service(rem),
                None => None,
            },
        }
    }

    fn find_service(&self, path: &str) -> Option<&Service> {
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

fn split_path<'a>(mut path: &'a str) -> Option<(&'a str, &'a str)> {
    path = path.trim_left_matches('/');

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

/// Convient implementation of `Service` which allows delegation to
/// multiple `Service` implementations provided by user.
#[derive(Default)]
pub struct ServicePaths {
    root: Node,
}

impl ServicePaths {
    /// Create a new `Service` implementation which returns `404`
    /// on all requests by default.
    pub fn new() -> ServicePaths {
        Default::default()
    }

    /// Register a service for given path.
    ///
    /// ```
    /// # use std::sync::Arc;
    /// use httpbis::*;
    /// use httpbis;
    ///
    /// struct Root {}
    /// struct Files {}
    ///
    /// impl Service for Root {
    ///     fn start_request(&self, _context: ServiceContext, _headers: Headers, _req: HttpStreamAfterHeaders, mut resp: ServerSender) -> httpbis::Result<()> {
    ///         resp.send_found_200_plain_text("This is root page")?;
    ///         Ok(())
    ///     }
    /// }
    ///
    /// impl Service for Files {
    ///     fn start_request(&self, _context: ServiceContext, _headers: Headers, _req: HttpStreamAfterHeaders, mut resp: ServerSender) -> httpbis::Result<()> {
    ///         resp.send_found_200_plain_text("This is files")?;
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let mut server = ServerBuilder::new_plain();
    /// server.service.set_service("/", Arc::new(Root{}));
    /// server.service.set_service("/files", Arc::new(Files{}));
    /// ```
    pub fn set_service(&mut self, path: &str, service: Arc<Service>) {
        assert!(path.starts_with("/"));
        self.root.add_service(path, service);
    }

    pub fn set_service_fn<F>(&mut self, path: &str, service: F)
    where
        F: Fn(ServiceContext, Headers, HttpStreamAfterHeaders, ServerSender) -> result::Result<()>
            + Send
            + Sync
            + 'static,
    {
        impl<
                F: Fn(ServiceContext, Headers, HttpStreamAfterHeaders, ServerSender)
                        -> result::Result<()>
                    + Send
                    + Sync
                    + 'static,
            > Service for F
        {
            fn start_request(
                &self,
                context: ServiceContext,
                headers: Headers,
                req: HttpStreamAfterHeaders,
                resp: ServerSender,
            ) -> result::Result<()> {
                self(context, headers, req, resp)
            }
        }

        self.set_service(path, Arc::new(service))
    }

    pub fn remove_service(&mut self, path: &str) -> Option<Arc<Service>> {
        assert!(path.starts_with("/"));
        self.root.remove_service(path)
    }

    fn find_service(&self, path: &str) -> Option<&Service> {
        self.root.find_service(path)
    }
}

impl Service for ServicePaths {
    fn start_request(
        &self,
        context: ServiceContext,
        headers: Headers,
        req: HttpStreamAfterHeaders,
        mut resp: ServerSender,
    ) -> result::Result<()> {
        if let Some(service) = self.find_service(headers.path()) {
            debug!("invoking user callback for path {}", headers.path());
            service.start_request(context, headers, req, resp)
        } else {
            debug!("serving 404 for path {}", headers.path());
            drop(resp.send_headers(Headers::not_found_404()));
            drop(resp.close());
            Ok(())
        }
    }
}
