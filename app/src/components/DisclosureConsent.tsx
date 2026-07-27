import { Pressable, StyleSheet, View } from 'react-native';
import { AppText } from '@/components';
import { colors } from '@/theme';

export function DisclosureConsent({ accepted, onChange }: { accepted: boolean; onChange(value: boolean): void }) {
  return <Pressable accessibilityRole="checkbox" accessibilityState={{ checked: accepted }} onPress={() => onChange(!accepted)} style={styles.row}><View style={[styles.box, accepted && styles.checked]}><AppText variant="label">{accepted ? '✓' : ''}</AppText></View><AppText variant="caption" color={colors.muted} style={styles.copy}>You are uploading a full platform export that may contain private content. Ghostpost opens allowlisted text only, sends minimized text to the approved model provider, deletes raw exports immediately after processing and no later than 24 hours, and keeps results as user-scoped server resources. Review actions affect Ghostpost only; we never edit live platform content.</AppText></Pressable>;
}
const styles = StyleSheet.create({ row: { flexDirection: 'row', gap: 10, alignItems: 'flex-start' }, box: { width: 24, height: 24, borderWidth: 1, borderColor: colors.muted, alignItems: 'center', justifyContent: 'center' }, checked: { backgroundColor: colors.accent }, copy: { flex: 1, lineHeight: 18 } });
