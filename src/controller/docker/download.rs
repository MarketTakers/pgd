use miette::{IntoDiagnostic, Result};
use std::{collections::HashMap, fmt::Write, pin::pin};

use bollard::{errors::Error, secret::CreateImageInfo};
use futures::{Stream, TryStreamExt};
use indicatif::{MultiProgress, ProgressBar, ProgressState, ProgressStyle};

fn new_download_pb(multi: &MultiProgress, layer_id: &str) -> ProgressBar {
    let pb = multi.add(ProgressBar::new(0));
    pb.set_style(
ProgressStyle::with_template(&"{spinner:.green} [{elapsed_precise}] {msg} [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})".to_string())
.unwrap()
.with_key("eta", |state: &ProgressState, w: &mut dyn Write| {
    write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap()
})
.progress_chars("#>-"),
);
    pb.set_message(format!("Layer {}", layer_id));
    pb
}

fn download_drive_progress(pb: &mut ProgressBar, download_info: CreateImageInfo) {
    match download_info.progress_detail {
        Some(info) => match (info.current, info.total) {
            (None, None) => {
                pb.inc(1);
            }
            (current, total) => {
                if let Some(total) = total {
                    pb.set_length(total as u64);
                }
                if let Some(current) = current {
                    pb.set_position(current as u64);
                }

                if let (Some(current), Some(total)) = (current, total)
                    && (current == total)
                {
                    pb.finish_with_message("Completed!");
                }
            }
        },
        None => {
            // No progress detail, just show activity
            pb.tick();
        }
    }
}

fn download_check_for_error(
    layer_progress: &mut HashMap<String, ProgressBar>,
    download_info: &CreateImageInfo,
) -> Result<()> {
    if let Some(error_detail) = &download_info.error_detail {
        for (_, pb) in layer_progress.drain() {
            pb.finish_and_clear();
        }

        match (error_detail.code, &error_detail.message) {
            (None, Some(msg)) => miette::bail!("docker image download error: {}", msg),
            (Some(code), None) => miette::bail!("docker image download error: code {}", code),
            (Some(code), Some(msg)) => {
                miette::bail!(
                    "docker image download error: code {}, message: {}",
                    code,
                    msg
                )
            }
            _ => (),
        }
    }

    Ok(())
}

// sadly type ... = impl ... is unstable
pub async fn perform_download(
    multi: MultiProgress,
    chunks: impl Stream<Item = Result<CreateImageInfo, Error>>,
) -> Result<()> {
    let mut chunks = pin!(chunks);
    let mut layer_progress: HashMap<String, ProgressBar> = HashMap::new();

    while let Some(download_info) = chunks.try_next().await.into_diagnostic()? {
        download_check_for_error(&mut layer_progress, &download_info)?;

        let layer_id = download_info.id.as_deref().unwrap_or("unknown");

        // Get or create progress bar for this layer
        let pb = layer_progress
            .entry(layer_id.to_string())
            .or_insert_with(|| new_download_pb(&multi, layer_id));

        download_drive_progress(pb, download_info);
    }

    // Clean up any remaining progress bars
    for (_, pb) in layer_progress.drain() {
        pb.finish_and_clear();
    }

    Ok(())
}
