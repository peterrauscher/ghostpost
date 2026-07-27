import { Alert, Platform, StyleSheet, View } from 'react-native';
import { AppText, Button, Screen, SurfaceCard } from '@/components';
import { useAuth } from '@/providers/auth-provider';
import { colors } from '@/theme';

export default function ProfileScreen() {
  const auth = useAuth();
  const confirmLogout = () => {
    if (Platform.OS === 'web') {
      if (globalThis.confirm('Log out? Ghostpost will revoke this session before clearing local data.')) void auth.signOut().catch(() => globalThis.alert('Log out failed. You are still signed in; please retry.'));
      return;
    }
    Alert.alert('Log out?', 'Ghostpost will revoke this session before clearing local data.', [{ text: 'Cancel', style: 'cancel' }, { text: 'Log out', onPress: () => void auth.signOut().catch(() => Alert.alert('Could not revoke session', 'Retry, or sign out locally. A local-only sign-out may leave the server session active until it expires.', [{ text: 'Retry' }, { text: 'Sign out locally', style: 'destructive', onPress: () => void auth.localOnlySignOut() }])) }]);
  };
  const confirmDelete = () => {
    if (Platform.OS === 'web') {
      if (globalThis.confirm('Delete Ghostpost account? Access is revoked immediately and Ghostpost-held content is purged asynchronously.')) void auth.deleteAccount().catch((error: unknown) => globalThis.alert(`Deletion failed: ${error instanceof Error ? error.message : 'Please retry.'}`));
      return;
    }
    Alert.alert('Delete Ghostpost account?', 'Access is revoked immediately. Ghostpost-held content is purged asynchronously.', [{ text: 'Cancel', style: 'cancel' }, { text: 'Delete account', style: 'destructive', onPress: () => void auth.deleteAccount().catch((error: unknown) => Alert.alert('Deletion failed', error instanceof Error ? error.message : 'Please retry.')) }]);
  };
  return <Screen tone="tint" scroll><View style={styles.content}><AppText variant="title">profile</AppText><SurfaceCard style={styles.card}><AppText variant="caption" color={colors.muted}>User</AppText><AppText variant="body" weight="700">{auth.user?.name ?? '—'}</AppText>{auth.user?.email ? <AppText variant="caption" color={colors.muted}>{auth.user.email}</AppText> : null}<AppText variant="caption" color={colors.muted}>id: {auth.user?.id ?? '—'}</AppText></SurfaceCard><SurfaceCard style={styles.card}><AppText variant="bodyRegular">Review actions affect your Ghostpost copy only. Ghostpost never edits or deletes live platform content.</AppText></SurfaceCard></View><View style={styles.footer}><Button label="log out" onPress={confirmLogout} /><Button label="delete Ghostpost account" variant="dark" onPress={confirmDelete} /></View></Screen>;
}
const styles = StyleSheet.create({ content: { gap: 14, paddingTop: 8, flex: 1 }, card: { gap: 6 }, footer: { paddingBottom: 12, paddingTop: 10, gap: 10 } });
