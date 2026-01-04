//! I/O primitives for communicating with the Claude CLI subprocess.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};

use crate::protocol::CliMessage;
use crate::{Error, Result};

/// Reads newline-delimited JSON messages from the CLI stdout.
///
/// The CLI outputs one JSON message per line. This reader handles buffering
/// and parsing each line into a [`CliMessage`].
pub struct ProcessReader {
    reader: BufReader<ChildStdout>,
    buffer: String,
}

impl ProcessReader {
    /// Create a new reader from a child process stdout.
    pub fn new(stdout: ChildStdout) -> Self {
        Self {
            reader: BufReader::new(stdout),
            buffer: String::with_capacity(4096),
        }
    }

    /// Read the next JSON message from the CLI.
    ///
    /// Returns `Ok(Some(message))` for each message, `Ok(None)` when EOF is reached,
    /// or `Err` on I/O or parse errors.
    pub async fn read_message(&mut self) -> Result<Option<CliMessage>> {
        loop {
            self.buffer.clear();

            let bytes_read = self
                .reader
                .read_line(&mut self.buffer)
                .await
                .map_err(Error::io)?;

            if bytes_read == 0 {
                return Ok(None);
            }

            // Trim the newline
            let line = self.buffer.trim();
            if line.is_empty() {
                // Empty line, continue to next iteration
                continue;
            }

            // Parse the JSON
            let message: CliMessage =
                serde_json::from_str(line).map_err(|e| Error::json_parse(e, line))?;

            return Ok(Some(message));
        }
    }

    /// Read messages as an async iterator.
    ///
    /// This returns a stream that yields messages until EOF or error.
    pub fn into_stream(self) -> MessageStream {
        MessageStream { reader: self }
    }
}

/// An async stream of CLI messages.
///
/// Created by [`ProcessReader::into_stream`].
pub struct MessageStream {
    reader: ProcessReader,
}

impl MessageStream {
    /// Get the next message from the stream.
    ///
    /// Returns `None` when the stream is exhausted.
    pub async fn next(&mut self) -> Option<Result<CliMessage>> {
        match self.reader.read_message().await {
            Ok(Some(msg)) => Some(Ok(msg)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

/// Writes prompts to the CLI stdin.
///
/// Used when prompts are too long to pass via command line argument.
pub struct ProcessWriter {
    stdin: ChildStdin,
}

impl ProcessWriter {
    /// Create a new writer from a child process stdin.
    pub fn new(stdin: ChildStdin) -> Self {
        Self { stdin }
    }

    /// Write a prompt to the CLI stdin and close it.
    ///
    /// The prompt is written as raw bytes followed by closing the stdin,
    /// which signals to the CLI that input is complete.
    pub async fn write_prompt(mut self, prompt: &str) -> Result<()> {
        self.stdin
            .write_all(prompt.as_bytes())
            .await
            .map_err(Error::io)?;
        self.stdin.shutdown().await.map_err(Error::io)?;
        Ok(())
    }
}

/// Reads stderr output from the CLI process.
///
/// Stderr typically contains debug logs and progress information.
/// This reader collects all stderr output for error reporting.
pub struct StderrReader {
    reader: BufReader<ChildStderr>,
}

impl StderrReader {
    /// Create a new stderr reader.
    pub fn new(stderr: ChildStderr) -> Self {
        Self {
            reader: BufReader::new(stderr),
        }
    }

    /// Read all remaining stderr output.
    ///
    /// This consumes the reader and returns all collected output.
    pub async fn read_all(mut self) -> Result<String> {
        let mut output = String::new();
        loop {
            let mut line = String::new();
            let bytes = self.reader.read_line(&mut line).await.map_err(Error::io)?;
            if bytes == 0 {
                break;
            }
            output.push_str(&line);
        }
        Ok(output)
    }

    /// Read the next line from stderr.
    pub async fn read_line(&mut self) -> Result<Option<String>> {
        let mut line = String::new();
        let bytes = self.reader.read_line(&mut line).await.map_err(Error::io)?;
        if bytes == 0 {
            Ok(None)
        } else {
            Ok(Some(line))
        }
    }
}

/// Wrapper for async polling of the message stream.
///
/// This implements the futures Stream trait for use with async combinators.
pub struct CliMessageStream {
    reader: Option<ProcessReader>,
    pending: Option<Pin<Box<dyn Future<Output = (ProcessReader, Result<Option<CliMessage>>)> + Send>>>,
}

impl CliMessageStream {
    /// Create a new stream from a process reader.
    pub fn new(reader: ProcessReader) -> Self {
        Self {
            reader: Some(reader),
            pending: None,
        }
    }
}

impl futures::Stream for CliMessageStream {
    type Item = Result<CliMessage>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // If we have a pending future, poll it
        if let Some(ref mut pending) = self.pending {
            match pending.as_mut().poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready((reader, result)) => {
                    self.pending = None;
                    self.reader = Some(reader);
                    match result {
                        Ok(Some(msg)) => return Poll::Ready(Some(Ok(msg))),
                        Ok(None) => return Poll::Ready(None),
                        Err(e) => return Poll::Ready(Some(Err(e))),
                    }
                }
            }
        }

        // Take the reader and create a new read future
        if let Some(mut reader) = self.reader.take() {
            let fut = Box::pin(async move {
                let result = reader.read_message().await;
                (reader, result)
            });
            self.pending = Some(fut);
            // Poll the new future immediately
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }

        // No reader available, stream is exhausted
        Poll::Ready(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_reader_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ProcessReader>();
    }

    #[test]
    fn cli_message_stream_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<CliMessageStream>();
    }
}
