use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Route {
    pub id: String,

    pub host: String,

    pub backend: BackendTarget,

    pub tls: bool,
}

#[derive(Debug, Clone)]
pub struct BackendTarget {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Serialize)]
struct Dynamic {
    http: Http,
}

#[derive(Debug, Serialize)]
struct Http {
    routers: BTreeMap<String, Router>,
    services: BTreeMap<String, Service>,
}

#[derive(Debug, Serialize)]
struct Router {
    rule: String,
    service: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    entryPoints: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tls: Option<RouterTls>,
}

#[derive(Debug, Serialize)]
struct RouterTls {
    certResolver: String,
}

#[derive(Debug, Serialize)]
struct Service {
    loadBalancer: LoadBalancer,
}

#[derive(Debug, Serialize)]
struct LoadBalancer {
    servers: Vec<Server>,
}

#[derive(Debug, Serialize)]
struct Server {
    url: String,
}

pub fn render_dynamic_yaml(routes: &[Route]) -> String {
    let mut routers = BTreeMap::new();
    let mut services = BTreeMap::new();

    for route in routes {
        let router_name = sanitize_name(&route.id);
        let service_name = format!("{router_name}-svc");

        let mut entrypoints = Vec::new();
        if route.tls {
            entrypoints.push("websecure".to_string());
        } else {
            entrypoints.push("web".to_string());
        }

        routers.insert(
            router_name.clone(),
            Router {
                rule: format!("Host(`{}`)", route.host.replace('`', "")),
                service: service_name.clone(),
                entryPoints: entrypoints,
                tls: route.tls.then(|| RouterTls {
                    certResolver: "le".to_string(),
                }),
            },
        );

        services.insert(
            service_name,
            Service {
                loadBalancer: LoadBalancer {
                    servers: vec![Server {
                        url: format!("http://{}:{}", route.backend.host, route.backend.port),
                    }],
                },
            },
        );
    }

    let dynamic = Dynamic {
        http: Http { routers, services },
    };

    serde_yaml::to_string(&dynamic).expect("yaml render")
}

fn sanitize_name(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_routes_produce_valid_yaml() {
        let yaml = render_dynamic_yaml(&[]);
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        assert!(parsed.get("http").is_some());
    }

    #[test]
    fn route_yaml_contains_expected_shape() {
        let routes = vec![Route {
            id: "blog-prod".to_string(),
            host: "blog.me.com".to_string(),
            backend: BackendTarget {
                host: "arx-blog-prod".to_string(),
                port: 3000,
            },
            tls: true,
        }];
        let yaml = render_dynamic_yaml(&routes);
        let v: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        let routers = &v["http"]["routers"];
        assert_eq!(routers["blog-prod"]["rule"], "Host(`blog.me.com`)");
        assert_eq!(routers["blog-prod"]["tls"]["certResolver"], "le");
        let services = &v["http"]["services"];
        assert_eq!(
            services["blog-prod-svc"]["loadBalancer"]["servers"][0]["url"],
            "http://arx-blog-prod:3000"
        );
    }

    #[test]
    fn render_is_deterministic() {
        let r1 = Route {
            id: "a".to_string(),
            host: "a.example".to_string(),
            backend: BackendTarget {
                host: "ca".to_string(),
                port: 80,
            },
            tls: false,
        };
        let r2 = Route {
            id: "b".to_string(),
            host: "b.example".to_string(),
            backend: BackendTarget {
                host: "cb".to_string(),
                port: 80,
            },
            tls: false,
        };
        let a = render_dynamic_yaml(&[r1.clone(), r2.clone()]);
        let b = render_dynamic_yaml(&[r2, r1]);
        assert_eq!(a, b);
    }

    #[test]
    fn id_is_sanitized() {
        let routes = vec![Route {
            id: "svc/with/slashes".to_string(),
            host: "x.example".to_string(),
            backend: BackendTarget {
                host: "c".to_string(),
                port: 80,
            },
            tls: false,
        }];
        let yaml = render_dynamic_yaml(&routes);
        assert!(yaml.contains("svc-with-slashes"));
    }

    #[test]
    fn host_backticks_stripped() {
        let routes = vec![Route {
            id: "x".to_string(),
            host: "evil`Host(`bad.com`)`".to_string(),
            backend: BackendTarget {
                host: "c".to_string(),
                port: 80,
            },
            tls: false,
        }];
        let yaml = render_dynamic_yaml(&routes);
        assert!(!yaml.contains("`bad.com`"));
    }
}
