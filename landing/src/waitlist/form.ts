import { parseContact } from '../lib/contact';

type State = 'idle' | 'sending' | 'done' | 'error';

/** Enhances a WaitlistForm: validates locally, posts as JSON-accepting fetch, and shows the result inline. */
export function mountWaitlistForm(form: HTMLFormElement) {
  const input = form.querySelector<HTMLInputElement>('input[name="contact"]');
  const status = form.querySelector<HTMLElement>('[data-waitlist-status]');
  if (!input || !status) throw new Error('waitlist form: missing input or status');

  const show = (state: State, message: string) => {
    form.dataset.state = state;
    status.textContent = message;
    input.setAttribute('aria-invalid', String(state === 'error'));
  };

  // Phone-looking input gets a numeric keypad on the next focus; emails keep the @ keyboard.
  input.addEventListener('input', () => {
    input.inputMode = /^[\d\s()+.-]+$/.test(input.value) ? 'tel' : 'email';
    if (form.dataset.state === 'error') show('idle', '');
  });

  form.addEventListener('submit', async (event) => {
    event.preventDefault();
    if (form.dataset.state === 'sending') return;
    const contact = parseContact(input.value);
    if (!contact) {
      show('error', 'that doesn’t look like an email or phone number.');
      input.focus();
      return;
    }

    show('sending', 'saving your spot…');
    try {
      const response = await fetch(form.action, {
        method: 'POST',
        body: new FormData(form),
        headers: { Accept: 'application/json' },
      });
      const body: unknown = await response.json().catch(() => null);
      if (response.ok) {
        show('done', contact.kind === 'email' ? 'you’re on the list 👻 we’ll email you.' : 'you’re on the list 👻 we’ll text you.');
        return;
      }
      const error = body && typeof body === 'object' && 'error' in body && typeof body.error === 'string' ? body.error : null;
      show('error', error ?? 'something went wrong. try again?');
    } catch {
      show('error', 'couldn’t reach us. check your connection and try again.');
    }
  });
}
