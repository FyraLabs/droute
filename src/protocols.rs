pub struct DRoute;

#[zbus::interface(
    name = "com.fyralabs.DRoute",
    proxy(
        gen_blocking = false,
        default_path = "/com/fyralabs/DRoute",
        default_service = "com.fyralabs.DRoute",
    )
)]
impl DRoute {
    fn query_interface(&self, interface: &str) -> String {
        "/".to_string() + &interface.replace('.', "/")
    }
}
