import * as DocumentPicker from 'expo-document-picker';
import { useState } from 'react';
import { AppText, Button, SurfaceCard } from '@/components';
import { colors } from '@/theme';

export function ArchivePicker({ onPicked, busy }: { onPicked(asset: DocumentPicker.DocumentPickerAsset): Promise<void>; busy?: boolean }) {
  const [error, setError] = useState<string | null>(null);
  return <SurfaceCard style={{ gap: 8 }}><AppText variant="body" weight="600">select your platform ZIP export</AppText><AppText variant="caption" color={colors.muted}>The archive uploads directly to private object storage. Ghostpost does not read it into app memory.</AppText>{error ? <AppText variant="caption" color={colors.accentDeep}>{error}</AppText> : null}<Button label="choose ZIP →" loading={busy} onPress={() => void (async () => { setError(null); const result = await DocumentPicker.getDocumentAsync({ type: ['application/zip','application/x-zip-compressed'], copyToCacheDirectory: true, multiple: false }); if (result.canceled) return; const asset = result.assets[0]; if (!asset?.size) { setError('The selected file has no readable size.'); return; } await onPicked(asset); })().catch((cause: unknown) => setError(cause instanceof Error ? cause.message : 'Upload failed.'))} /></SurfaceCard>;
}
