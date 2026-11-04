#[path = "../cli.rs"]
mod cli;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _diagnostics = httpied::diagnostics::start()?;
    let result = cli::run();
    if let Err(error) = &result {
        tracing::error!(error = %error, error_debug = ?error, "application exited with an error");
    }
    result
}
