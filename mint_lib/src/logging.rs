use std::io::BufWriter;
use std::path::Path;

use fs_err as fs;
use tracing::*;
use tracing_subscriber::fmt::format::FmtSpan;

use crate::MintError;

pub fn setup_logging<P: AsRef<Path>>(
    log_path: P,
    target: &str,
) -> Result<tracing_appender::non_blocking::WorkerGuard, MintError> {
    use tracing::metadata::LevelFilter;
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{
        field::RecordFields,
        filter,
        fmt::{
            self,
            format::{Pretty, Writer},
            FormatFields,
        },
        EnvFilter,
    };

    /// Workaround for <https://github.com/tokio-rs/tracing/issues/1817>.
    struct NewType(Pretty);

    impl<'writer> FormatFields<'writer> for NewType {
        fn format_fields<R: RecordFields>(
            &self,
            writer: Writer<'writer>,
            fields: R,
        ) -> core::fmt::Result {
            self.0.format_fields(writer, fields)
        }
    }

    let f = fs::File::create(log_path.as_ref()).map_err(|_| MintError::LogSetupFailed {
        summary: "failed to create log file".to_string(),
        details: Some(format!("log file path: `{}`", log_path.as_ref().display())),
    })?;

    let writer = BufWriter::new(f);
    let (log_file_appender, guard) = tracing_appender::non_blocking(writer);
    let debug_file_log = fmt::layer()
        .with_writer(log_file_appender)
        .fmt_fields(NewType(Pretty::default()))
        .with_ansi(false)
        .with_filter(filter::Targets::new().with_target(target, Level::DEBUG));
    let stderr_log = fmt::layer()
        .with_writer(std::io::stderr)
        .event_format(tracing_subscriber::fmt::format().without_time())
        .with_span_events(FmtSpan::CLOSE)
        .with_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        );
    let subscriber = tracing_subscriber::registry()
        .with(stderr_log)
        .with(debug_file_log);

    tracing::subscriber::set_global_default(subscriber).map_err(|e| MintError::LogSetupFailed {
        summary: "failed to register global default tracing subscriber".to_string(),
        details: Some(format!("{e}")),
    })?;

    debug!("tracing subscriber setup");
    info!("writing logs to {:?}", log_path.as_ref().display());

    Ok(guard)
}
