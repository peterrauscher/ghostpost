import * as Crypto from 'expo-crypto';
import * as DocumentPicker from 'expo-document-picker';
import { Image } from 'expo-image';
import { router } from 'expo-router';
import { ArrowRight, FileUp, Plus, ShieldCheck } from 'lucide-react-native';
import { useRef, useState } from 'react';
import {
  ActivityIndicator,
  Pressable,
  StyleSheet,
  useWindowDimensions,
  View,
} from 'react-native';

import { AppText, Button, Screen, SurfaceCard } from '@/components';
import { DisclosureConsent } from '@/components/DisclosureConsent';
import { PlatformIcon } from '@/components/PlatformIcon';
import {
  useOnboardingQuery,
  useStartScanMutation,
  useSubmitOnboardingMutation,
} from '@/features/hooks';
import { useLifecycle } from '@/providers/lifecycle-provider';
import { api } from '@/services/api';
import { bodyFromPickerAsset, putArchiveBytes } from '@/services/api/archive-upload';
import { colors, radii } from '@/theme';

type SupportedPlatform = 'reddit' | 'x';

const supported: { id: SupportedPlatform; label: string }[] = [
  { id: 'x', label: 'X (Twitter)' },
  { id: 'reddit', label: 'Reddit' },
];

const upcoming = [
  { id: 'instagram', label: 'Instagram' },
  { id: 'tiktok', label: 'TikTok' },
  { id: 'facebook', label: 'Facebook' },
] as const;

type PillTone = 'ready' | 'error' | 'pending' | 'busy' | 'idle';

const pillTones: Record<PillTone, { backgroundColor: string; color: string }> = {
  ready: { backgroundColor: colors.riskLowBg, color: colors.riskLowText },
  error: { backgroundColor: colors.riskHighBg, color: colors.riskHighText },
  pending: { backgroundColor: colors.accentSoft, color: colors.accentDeep },
  busy: { backgroundColor: colors.riskMediumBg, color: colors.riskMediumText },
  idle: { backgroundColor: colors.tagBg, color: colors.muted },
};

function StatusPill({ label, tone }: { label: string; tone: PillTone }) {
  const palette = pillTones[tone];

  return (
    <View style={[styles.pill, { backgroundColor: palette.backgroundColor }]}>
      <AppText variant="label" color={palette.color}>
        {label}
      </AppText>
    </View>
  );
}

export default function ScanTabScreen() {
  const { width } = useWindowDimensions();
  const wide = width >= 720;
  const tight = width < 360;

  const onboarding = useOnboardingQuery();
  const lifecycle = useLifecycle();
  const startScan = useStartScanMutation();
  const save = useSubmitOnboardingMutation();
  const [busy, setBusy] = useState<SupportedPlatform | null>(null);
  const [messages, setMessages] = useState<Partial<Record<SupportedPlatform, string>>>({});
  const [errors, setErrors] = useState<Partial<Record<SupportedPlatform, string>>>({});
  const [actionError, setActionError] = useState<string | null>(null);
  const [savingPlatform, setSavingPlatform] = useState<SupportedPlatform | null>(null);
  const [focused, setFocused] = useState<SupportedPlatform | null>(null);
  const operation = useRef(false);

  const selected = onboarding.data?.answers.platforms.filter(
    (id): id is SupportedPlatform => id === 'reddit' || id === 'x',
  ) ?? [];

  const consented = onboarding.data?.answers.disclosureConsent.accepted === true;

  const platformCards: { id: SupportedPlatform; label: string }[] = onboarding.data
    ? supported
    : [];

  const readyPlatforms = new Set(
    lifecycle.imports.filter((item) => item.status === 'ready').map((item) => item.platform),
  );

  const allReady = selected.length > 0 && selected.every((id) => readyPlatforms.has(id));

  const locked =
    busy !== null || save.isPending || startScan.isPending || onboarding.isPending || lifecycle.loading;

  const missing = selected.filter((id) => !readyPlatforms.has(id));

  const selectedProcessing = lifecycle.imports.some(
    (item) =>
      selected.some((id) => id === item.platform) &&
      ['uploaded', 'queued', 'normalizing'].includes(item.status),
  );

  const pendingUploads = missing.filter((id) =>
    lifecycle.imports.some(
      (item) =>
        item.platform === id &&
        ['uploaded', 'queued', 'normalizing'].includes(item.status),
    ),
  );

  let hint = 'Your selected archives are ready.';

  if (selected.length === 0) {
    hint = 'Add X or Reddit to include a platform in your scan.';
  } else if (!consented) {
    hint = 'Accept the privacy consent to continue.';
  } else if (selectedProcessing || pendingUploads.length > 0) {
    hint = 'Waiting for your archives to finish processing.';
  } else if (missing.length > 0) {
    const labels = missing.map((id) => supported.find((item) => item.id === id)?.label ?? id);
    hint = `Upload a ZIP for ${labels.join(' and ')} to start.`;
  }

  const upload = async (platform: SupportedPlatform) => {
    if (operation.current || locked || !consented) return;
    operation.current = true;
    setBusy(platform);
    setActionError(null);
    setErrors((current) => ({ ...current, [platform]: undefined }));
    setMessages((current) => ({ ...current, [platform]: 'Choose your ZIP export…' }));

    try {
      const result = await DocumentPicker.getDocumentAsync({
        type: ['application/zip', 'application/x-zip-compressed'],
        copyToCacheDirectory: true,
        multiple: false,
      });

      if (result.canceled) return;
      const asset = result.assets[0];

      if (!asset?.size) throw new Error('The selected file has no readable size. Choose another ZIP.');
      setMessages((current) => ({ ...current, [platform]: 'Reserving private upload…' }));

      const reserve = await api.reserveArchiveImport(
        { platform, contentLength: asset.size, contentType: asset.mimeType || 'application/zip' },
        Crypto.randomUUID(),
      );

      setMessages((current) => ({ ...current, [platform]: 'Uploading directly to private storage…' }));
      await putArchiveBytes(reserve.upload, bodyFromPickerAsset(asset));
      setMessages((current) => ({ ...current, [platform]: 'Verifying and processing archive…' }));
      await api.completeArchiveImport(reserve.id, Crypto.randomUUID());

      try {
        await lifecycle.refetch();
      } catch {
        setActionError(
          'Your archive uploaded, but refreshing its status failed. It will update after your next visit.',
        );
      }
    } catch (cause: unknown) {
      setErrors((current) => ({
        ...current,
        [platform]:
          cause instanceof Error ? cause.message : 'Upload failed. Choose your ZIP to retry.',
      }));
    } finally {
      setMessages((current) => ({ ...current, [platform]: undefined }));
      setBusy(null);
      operation.current = false;
    }
  };

  const addPlatform = async (platform: SupportedPlatform) => {
    if (!onboarding.data || operation.current || locked) return;
    operation.current = true;
    setSavingPlatform(platform);
    setActionError(null);

    try {
      await save.mutateAsync({
        ...onboarding.data,
        answers: {
          ...onboarding.data.answers,
          platforms: [...new Set([...onboarding.data.answers.platforms, platform])],
        },
      });
      await lifecycle.refetch();
    } catch (cause: unknown) {
      setActionError(
        cause instanceof Error ? cause.message : 'Could not add the platform. Please try again.',
      );
    } finally {
      setSavingPlatform(null);
      operation.current = false;
    }
  };

  const acceptDisclosure = async (accepted: boolean) => {
    if (!onboarding.data || operation.current || locked) return;
    operation.current = true;
    setActionError(null);

    try {
      await save.mutateAsync({
        ...onboarding.data,
        answers: {
          ...onboarding.data.answers,
          disclosureConsent: { ...onboarding.data.answers.disclosureConsent, accepted },
        },
      });
      await lifecycle.refetch();
    } catch (cause: unknown) {
      setActionError(
        cause instanceof Error ? cause.message : 'Could not save consent. Please try again.',
      );
    } finally {
      operation.current = false;
    }
  };

  const begin = async () => {
    if (!allReady || !consented || locked || operation.current || selectedProcessing) return;
    operation.current = true;
    setActionError(null);

    try {
      const ids = lifecycle.imports
        .filter(
          (item) =>
            item.status === 'ready' && selected.some((platform) => platform === item.platform),
        )
        .map((item) => item.id);

      const scan = await startScan.mutateAsync(ids);
      router.replace({ pathname: '/scan', params: { scanId: scan.id } });
    } catch (cause: unknown) {
      setActionError(
        cause instanceof Error ? cause.message : 'Could not start your scan. Please try again.',
      );
    } finally {
      operation.current = false;
    }
  };

  return (
    <Screen tone="welcome" scroll>
      <View style={[styles.page, wide && styles.pageWide]}>
        <View style={styles.header}>
          <View style={[styles.hero, wide && styles.heroWide]}>
            <View style={styles.heroCopy}>
              <View accessible accessibilityRole="header">
                <AppText
                  variant="display"
                  style={[styles.title, tight && styles.titleTight, wide && styles.titleWide]}>
                  scan your
                </AppText>
                <AppText
                  variant="display"
                  color={colors.accentDeep}
                  style={[styles.title, tight && styles.titleTight, wide && styles.titleWide]}>
                  social data
                </AppText>
              </View>
            </View>
            <View
              pointerEvents="none"
              accessibilityElementsHidden
              importantForAccessibility="no-hide-descendants"
              style={[styles.art, wide && styles.artWide]}>
              <Image
                source={require('@/assets/images/ghosts/scan-upload-ghost.png')}
                contentFit="contain"
                style={styles.ghost}
              />
            </View>
          </View>
          <AppText color={colors.muted} style={styles.subtitle}>
            Upload your exported data so we can analyze your posts, comments, and more.
          </AppText>
        </View>

        {onboarding.isPending || lifecycle.loading ? (
          <View style={styles.loading}>
            <ActivityIndicator color={colors.accent} />
            <AppText color={colors.muted}>Loading your archives…</AppText>
          </View>
        ) : null}
        {onboarding.isError ? (
          <SurfaceCard style={[styles.card, wide && styles.cardWide]}>
            <AppText accessibilityRole="alert">
              Could not load your platform selections.
            </AppText>
            <Button label="Retry loading" onPress={() => void onboarding.refetch()} />
          </SurfaceCard>
        ) : null}

        {platformCards.map(({ id, label }) => {
          const isSelected = selected.includes(id);
          const imports = lifecycle.imports.filter((item) => item.platform === id);
          const ready = imports.some((item) => item.status === 'ready');

          const processing = imports.some((item) =>
            ['uploaded', 'queued', 'normalizing'].includes(item.status),
          );

          const failed = !ready && !processing && imports.some((item) => item.status === 'failed');
          const error = errors[id];
          const uploading = busy === id;
          const disabled = locked || processing || !consented;

          const status = (
            uploading ? { label: 'Uploading', tone: 'busy' }
              : error ? { label: 'Try again', tone: 'error' }
                : processing ? { label: 'Processing', tone: 'busy' }
                  : ready ? { label: 'Ready to scan', tone: 'ready' }
                    : failed ? { label: 'Upload failed', tone: 'error' }
                      : !consented && isSelected ? { label: 'Consent needed', tone: 'pending' }
                        : isSelected ? { label: 'Needs export', tone: 'pending' }
                          : { label: 'Not selected', tone: 'idle' }
          ) satisfies { label: string; tone: PillTone };

          let dropzoneTitle = 'Choose file';
          let dropzoneHint = 'or tap to browse';
          let dropzoneCaption = `${label} data export (ZIP)`;

          if (uploading) {
            dropzoneTitle = messages[id] ?? 'Preparing your upload…';
            dropzoneHint = 'Keep this screen open while the upload runs.';
            dropzoneCaption = `${label} ZIP`;
          } else if (processing) {
            dropzoneTitle = 'Processing your archive…';
            dropzoneHint = 'This usually takes a couple of minutes.';
            dropzoneCaption = `${label} ZIP`;
          } else if (error || failed) {
            dropzoneTitle = 'Choose ZIP to retry';
            dropzoneCaption = `Try a fresh ${label} export.`;
          } else if (ready) {
            dropzoneTitle = 'Choose a replacement ZIP';
            dropzoneCaption = 'Your archive is ready for a private scan.';
          } else if (!consented) {
            dropzoneTitle = 'Consent required to upload';
            dropzoneHint = 'Use the privacy consent below.';
          }

          const showBrowseLine =
            !uploading && !processing && !error && !failed && !ready && consented;

          return (
            <SurfaceCard key={id} elevated={false} style={[styles.card, wide && styles.cardWide]}>
              <View style={styles.platformRow}>
                <PlatformIcon platform={id} size={38} />
                <View style={styles.platformCopy}>
                  <View style={styles.nameRow}>
                    <AppText variant="headline">{label}</AppText>
                    <StatusPill label={status.label} tone={status.tone} />
                  </View>
                  <AppText color={colors.muted} style={styles.platformNote}>
                    {isSelected
                      ? 'Upload your data export (ZIP file)'
                      : 'Add this platform to your private scan'}
                  </AppText>
                </View>
              </View>

              {isSelected ? (
                <Pressable
                  accessibilityRole="button"
                  accessibilityLabel={`${ready ? 'Replace' : 'Choose'} ${label} ZIP export`}
                  accessibilityHint={
                    !consented
                      ? 'Accept the privacy consent below to enable uploads'
                      : processing
                        ? 'Wait until the current archive finishes processing'
                        : 'Opens the file picker; the archive uploads directly to private storage'
                  }
                  accessibilityState={{ disabled, busy: uploading || processing }}
                  disabled={disabled}
                  onFocus={() => setFocused(id)}
                  onBlur={() => setFocused(null)}
                  onPress={() => void upload(id)}
                  style={({ pressed }) => [
                    styles.dropzone,
                    wide && styles.dropzoneWide,
                    focused === id && styles.focused,
                    pressed && styles.pressed,
                    disabled && styles.disabled,
                  ]}>
                  <View style={styles.dropzoneIcon}>
                    {uploading || processing ? (
                      <ActivityIndicator color={colors.accent} />
                    ) : (
                      <FileUp size={24} color={colors.accent} />
                    )}
                  </View>
                  {showBrowseLine ? (
                    <View style={styles.dropzoneRow}>
                      <AppText variant="body" color={colors.accentDeep}>
                        Choose file
                      </AppText>
                      <AppText variant="caption" color={colors.muted}>
                        or tap to browse
                      </AppText>
                    </View>
                  ) : (
                    <>
                      <AppText
                        variant="body"
                        color={colors.accentDeep}
                        align="center"
                        accessibilityLiveRegion="polite">
                        {dropzoneTitle}
                      </AppText>
                      <AppText variant="caption" color={colors.muted} align="center">
                        {dropzoneHint}
                      </AppText>
                    </>
                  )}
                  <AppText variant="caption" color={colors.muted} align="center">
                    {dropzoneCaption}
                  </AppText>
                </Pressable>
              ) : (
                <Button
                  label={`Add ${label}`}
                  icon={Plus}
                  disabled={locked || !onboarding.data}
                  loading={savingPlatform === id}
                  onPress={() => void addPlatform(id)}
                />
              )}

              {!uploading && (error || failed) ? (
                <AppText accessibilityRole="alert" color={colors.riskHighText}>
                  {error ??
                    `This archive could not be processed. Choose a fresh ${label} export to retry.`}
                </AppText>
              ) : null}
            </SurfaceCard>
          );
        })}

        {upcoming.map(({ id, label }) => (
          <SurfaceCard
            key={id}
            elevated={false}
            style={[styles.compactCard, wide && styles.cardWide]}>
            <PlatformIcon platform={id} size={32} />
            <View style={styles.moreCopy}>
              <View style={styles.nameRow}>
                <AppText variant="headline">{label}</AppText>
                <StatusPill label="Not connected" tone="busy" />
              </View>
              <AppText color={colors.muted} style={styles.platformNote}>
                Archive support is on the way
              </AppText>
            </View>
            <StatusPill label="Coming soon" tone="pending" />
          </SurfaceCard>
        ))}

        <View style={styles.scanAction}>
          <View style={styles.privacyTitle}>
            <ShieldCheck size={18} color={colors.accentDeep} />
            <AppText variant="body">Your data stays private</AppText>
          </View>
          <AppText variant="caption" color={colors.muted}>
            Full exports may contain private content. Ghostpost opens allowlisted text only and
            sends minimized text to the approved model provider. Raw archives are deleted
            immediately after processing, with a 24-hour fallback. Results stay in your account.
            Review actions never edit live platform content.
          </AppText>
          {onboarding.data && !consented ? (
            <View
              style={locked ? styles.consentLocked : undefined}
              pointerEvents={locked ? 'none' : 'auto'}>
              <DisclosureConsent
                accepted={consented}
                onChange={(accepted) => void acceptDisclosure(accepted)}
              />
            </View>
          ) : null}
          {actionError ? (
            <AppText accessibilityRole="alert" color={colors.riskHighText}>
              {actionError}
            </AppText>
          ) : null}
          <Button
            label="Start private scan"
            icon={ArrowRight}
            disabled={!allReady || !consented || locked || selectedProcessing}
            loading={startScan.isPending}
            onPress={() => void begin()}
          />
          <AppText variant="caption" align="center" color={colors.muted}>
            {hint}
          </AppText>
        </View>
      </View>
    </Screen>
  );
}

const styles = StyleSheet.create({
  page: { gap: 18, paddingTop: 18, paddingBottom: 28 },
  pageWide: { gap: 20, paddingTop: 32 },
  header: { gap: 8 },
  hero: { flexDirection: 'row', alignItems: 'center', minHeight: 118 },
  heroWide: { minHeight: 140 },
  heroCopy: { flex: 1, minWidth: 0, zIndex: 1 },
  title: { fontSize: 34, lineHeight: 37, letterSpacing: -1.2 },
  titleTight: { fontSize: 31, lineHeight: 34 },
  titleWide: { fontSize: 38, lineHeight: 42 },
  art: { width: 104, height: 118, marginLeft: 4 },
  artWide: { width: 124, height: 132 },
  ghost: { width: '100%', height: '100%' },
  subtitle: { fontSize: 15, lineHeight: 22, maxWidth: 276 },
  card: { borderWidth: 0, borderRadius: 20, padding: 14, gap: 12 },
  cardWide: { padding: 18 },
  platformRow: { flexDirection: 'row', alignItems: 'center', gap: 12 },
  platformCopy: { flex: 1, minWidth: 0, gap: 4 },
  nameRow: { flexDirection: 'row', flexWrap: 'wrap', alignItems: 'center', gap: 8 },
  platformNote: { fontSize: 13, lineHeight: 18 },
  pill: { borderRadius: radii.full, paddingHorizontal: 9, paddingVertical: 5 },
  dropzone: {
    borderWidth: 1.5,
    borderStyle: 'dashed',
    borderColor: colors.carouselInactive,
    borderRadius: 12,
    backgroundColor: colors.bg,
    paddingHorizontal: 14,
    paddingVertical: 12,
    alignItems: 'center',
    justifyContent: 'center',
    gap: 2,
  },
  dropzoneWide: { paddingVertical: 16 },
  dropzoneIcon: { height: 24, justifyContent: 'center', marginBottom: 2 },
  dropzoneRow: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    flexWrap: 'wrap',
    gap: 5,
  },
  focused: { borderStyle: 'solid', borderWidth: 2, borderColor: colors.accentDeep },
  pressed: { backgroundColor: colors.progressTrack },
  disabled: { opacity: 0.65 },
  compactCard: {
    borderWidth: 0,
    borderRadius: 20,
    paddingHorizontal: 14,
    paddingVertical: 10,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
  },
  moreCopy: { flex: 1, minWidth: 0, gap: 2 },
  scanAction: { gap: 12, paddingHorizontal: 4, paddingTop: 6 },
  privacyTitle: { flexDirection: 'row', alignItems: 'center', gap: 8 },
  consentLocked: { opacity: 0.6 },
  loading: { flexDirection: 'row', alignItems: 'center', gap: 8, paddingVertical: 8 },
});
