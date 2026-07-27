import * as Crypto from 'expo-crypto';
import type * as DocumentPicker from 'expo-document-picker';
import { router } from 'expo-router';
import { useState } from 'react';
import { View } from 'react-native';
import { AppText, Button, Screen, SurfaceCard } from '@/components';
import { ArchivePicker } from '@/components/ArchivePicker';
import { useOnboardingQuery, useStartScanMutation } from '@/features/hooks';
import { useLifecycle } from '@/providers/lifecycle-provider';
import { api } from '@/services/api';
import { bodyFromPickerAsset, postArchiveBytes } from '@/services/api/archive-upload';
import type { PlatformId } from '@/domain/types';
import { colors } from '@/theme';

export default function ScanTabScreen() {
  const onboarding = useOnboardingQuery(); const lifecycle = useLifecycle(); const startScan = useStartScanMutation();
  const selected = onboarding.data?.answers.platforms.filter((id): id is 'reddit'|'x' => id === 'reddit' || id === 'x') ?? [];
  const readyPlatforms = new Set(lifecycle.imports.filter((item) => item.status === 'ready').map((item) => item.platform));
  const nextPlatform = selected.find((platform) => !readyPlatforms.has(platform));
  const [platform, setPlatform] = useState<PlatformId | null>(null); const [busy, setBusy] = useState(false); const [message, setMessage] = useState<string | null>(null);
  const chosen = platform ?? nextPlatform ?? selected[0];

  const upload = async (asset: DocumentPicker.DocumentPickerAsset) => {
    if (!chosen || !asset.size) throw new Error('Choose Reddit or X before uploading.');
    setBusy(true); setMessage('reserving private upload…');
    try {
      const reserve = await api.reserveArchiveImport({ platform: chosen, contentLength: asset.size, contentType: asset.mimeType || 'application/zip' }, Crypto.randomUUID());
      setMessage('uploading directly to private storage…');
      await postArchiveBytes(reserve.upload, bodyFromPickerAsset(asset), asset.name);
      setMessage('verifying and processing archive…');
      await api.completeArchiveImport(reserve.id, Crypto.randomUUID());
      await lifecycle.refetch();
    } finally { setBusy(false); }
  };
  const begin = async () => { const ids = lifecycle.imports.filter((item) => item.status === 'ready' && selected.includes(item.platform as 'reddit'|'x')).map((item) => item.id); const scan = await startScan.mutateAsync(ids); router.replace({ pathname: '/scan', params: { scanId: scan.id } }); };
  const allReady = selected.length > 0 && selected.every((id) => readyPlatforms.has(id));
  return <Screen tone="welcome" scroll><View style={{ gap: 14 }}><AppText variant="title">upload and scan</AppText><AppText variant="bodyRegular" color={colors.muted}>Upload each selected platform ZIP export. Raw archives are deleted immediately after processing, with a 24-hour fallback.</AppText><View style={{ flexDirection: 'row', gap: 8 }}>{selected.map((id) => <Button key={id} label={`${readyPlatforms.has(id) ? '✓ ' : ''}${id}`} variant={chosen === id ? 'dark' : undefined} onPress={() => setPlatform(id)} />)}</View>{!allReady ? <ArchivePicker onPicked={upload} busy={busy} /> : <SurfaceCard><AppText variant="body">all selected archives are ready</AppText></SurfaceCard>}{message ? <AppText variant="caption" color={colors.muted}>{message}</AppText> : null}<Button label="start private scan →" disabled={!allReady} loading={startScan.isPending} onPress={() => void begin()} /></View></Screen>;
}
