import { File as ExpoFile, UploadType } from 'expo-file-system';
import { Platform } from 'react-native';
import type * as DocumentPicker from 'expo-document-picker';
import type { SignedPut } from './types';

export type ArchiveUploadBody = ExpoFile | Blob;

export function bodyFromPickerAsset(asset: DocumentPicker.DocumentPickerAsset): ArchiveUploadBody {
  if (Platform.OS === 'web') {
    if (!asset.file) throw new Error('Browser picker did not return a File');
    return asset.file;
  }
  return new ExpoFile(asset.uri);
}

/**
 * Uploads the archive straight to object storage with the presigned PUT.
 * The signature binds the exact byte length, so the body must be sent as-is
 * with a fixed Content-Length: the browser derives it from the File, and the
 * native uploader streams the file from disk (never buffered in JS memory).
 * Every signed header must be sent verbatim.
 */
export async function putArchiveBytes(upload: SignedPut, body: ArchiveUploadBody): Promise<void> {
  if (Platform.OS !== 'web') {
    const result = await (body as ExpoFile).upload(upload.url, {
      httpMethod: 'PUT',
      uploadType: UploadType.BINARY_CONTENT,
      headers: upload.headers,
    });
    if (result.status < 200 || result.status >= 300) throw new Error(`Archive upload failed: ${result.status}`);
    return;
  }
  const response = await fetch(upload.url, { method: 'PUT', headers: upload.headers, body });
  if (!response.ok) throw new Error(`Archive upload failed: ${response.status}`);
}
