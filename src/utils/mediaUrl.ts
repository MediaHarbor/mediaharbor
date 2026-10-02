import { convertFileSrc } from '@tauri-apps/api/core';

export function toAudioUrl(url: string): string {
  if (!url) return url;
  if (/^https?:\/\/|^blob:/.test(url)) return url;

  let filePath = url;
  if (url.startsWith('file:///')) {
    filePath = decodeURIComponent(url.slice('file:///'.length));
  } else if (url.startsWith('file://')) {
    filePath = decodeURIComponent(url.slice('file://'.length));
  }
  const absPath = (filePath.startsWith('/') ? filePath : '/' + filePath).replace(/\\/g, '/');
  return convertFileSrc(absPath);
}
