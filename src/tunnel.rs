//! Opaque tunnels expire on inactivity, not while bytes continue to flow.
use std::{io, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::watch,
    time::{Instant, sleep_until},
};
pub async fn copy<
    A: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    B: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
>(
    client: &mut A,
    upstream: &mut B,
    idle: Duration,
) -> io::Result<(u64, u64)> {
    let (mut cr, mut cw) = tokio::io::split(client);
    let (mut ur, mut uw) = tokio::io::split(upstream);
    let (activity, mut changed) = watch::channel(Instant::now());
    let other = activity.clone();
    let up = async move {
        let mut buf = [0; 8192];
        let mut total = 0;
        loop {
            let n = cr.read(&mut buf).await?;
            if n == 0 {
                uw.shutdown().await?;
                break;
            }
            uw.write_all(&buf[..n]).await?;
            total += n as u64;
            activity.send_replace(Instant::now());
        }
        Ok::<_, io::Error>(total)
    };
    let down = async move {
        let mut buf = [0; 8192];
        let mut total = 0;
        loop {
            let n = ur.read(&mut buf).await?;
            if n == 0 {
                cw.shutdown().await?;
                break;
            }
            cw.write_all(&buf[..n]).await?;
            total += n as u64;
            other.send_replace(Instant::now());
        }
        Ok::<_, io::Error>(total)
    };
    let transfer = async { tokio::try_join!(up, down) };
    tokio::pin!(transfer);
    loop {
        let deadline = *changed.borrow() + idle;
        tokio::select! {result=&mut transfer=>return result,_=changed.changed()=>{},_=sleep_until(deadline)=>return Err(io::Error::new(io::ErrorKind::TimedOut,"CONNECT tunnel idle timeout"))}
    }
}
