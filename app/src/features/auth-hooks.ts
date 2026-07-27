import { useMutation } from '@tanstack/react-query';
import { useAuth } from '@/providers/auth-provider';

export function useSession() { return useAuth(); }
export function useLogout() { const auth = useAuth(); return useMutation({ mutationFn: auth.signOut }); }
export function useDeleteAccount() { const auth = useAuth(); return useMutation({ mutationFn: auth.deleteAccount }); }
