import { fetch as expoFetch } from 'expo/fetch';
import { File as ExpoFile } from 'expo-file-system';
import { Platform } from 'react-native';
import type * as DocumentPicker from 'expo-document-picker';
import type { SignedPost } from './types';

export type ArchiveUploadBody = ExpoFile | Blob;

export function bodyFromPickerAsset(asset: DocumentPicker.DocumentPickerAsset): ArchiveUploadBody {
  if (Platform.OS === 'web') {
    if (!asset.file) throw new Error('Browser picker did not return a File');
    return asset.file;
  }
  return new ExpoFile(asset.uri);
}

export async function postArchiveBytes(upload: SignedPost, body: ArchiveUploadBody, filename: string): Promise<void> {
  const form = new FormData();
  for (const [name, value] of Object.entries(upload.fields)) form.append(name, value);
  form.append('file', body as Blob, filename);
  const response = await expoFetch(upload.url, { method: 'POST', body: form });
  if (!response.ok) throw new Error(`Archive upload failed: ${response.status}`);
}
