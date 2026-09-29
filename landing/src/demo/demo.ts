import { DEMO_POSTS, auditHeadline, overallRisk, riskLabel, type DemoPost, type Platform } from './data';

type View = 'scan' | 'home' | 'detail';
type ReviewAction = 'delete' | 'archive' | 'keep' | 'resolve';
interface Run { cancelled: boolean }

const SCAN_MESSAGES: readonly [progress: number, message: string][] = [
  [0, 'unpacking your exports…'],
  [0.3, 'reading 1,284 posts…'],
  [0.65, 'checking for red flags…'],
  [1, 'all done! 🎉'],
];

const TOASTS: Record<ReviewAction, string> = {
  delete: 'removed from ghostpost 🧹',
  archive: 'archived for later 📦',
  keep: 'kept. own it ✨',
  resolve: 'marked as resolved ✅',
};

class Cancelled extends Error {}

/**
 * Drives the phone mockup rendered by PhoneDemo.astro. Autoplays a scan → review → clean-up loop
 * while on screen; the first real tap or key press hands control to the visitor.
 */
export function mountDemo(root: HTMLElement) {
  const $ = <T extends Element = HTMLElement>(selector: string): T => {
    const el = root.querySelector<T>(selector);
    if (!el) throw new Error(`demo: missing ${selector}`);
    return el;
  };
  const field = (name: string) => $(`[data-field="${name}"]`);

  const screen = $('.d-screen');
  const home = $('.d-home');
  const detail = $('.d-detail');
  const list = $('[data-list]');
  const flagged = $('.d-flagged');
  const riskCard = $('.d-risk');
  const gauge = field('gauge');
  const finger = $('[data-finger]');
  const toast = field('toast');
  const scanFill = field('scan-progress');
  const scanMessage = field('scan-message');
  const scanPlatforms = [...root.querySelectorAll<HTMLElement>('.d-scan__platform')];
  const icons = JSON.parse(root.dataset.icons ?? '{}') as Record<Platform, string>;
  const pristineRows = [...list.children].map((row) => row.cloneNode(true));
  const circumference = Number(gauge.dataset.circumference);

  let posts: DemoPost[] = [...DEMO_POSTS];
  let current: DemoPost | null = null;
  let scanProgress = 0;
  let run: Run = { cancelled: false };
  let onScreen = false;
  let toastTimer = 0;

  const setView = (view: View) => {
    screen.dataset.view = view;
    home.inert = view !== 'home';
    detail.inert = view !== 'detail';
  };

  // Autoplay time only advances while the phone is visible, so nothing plays to an empty room.
  const sleep = async (ms: number, owner: Run) => {
    let left = ms;
    while (left > 0) {
      const step = Math.min(left, 80);
      const { promise, resolve } = Promise.withResolvers<void>();
      setTimeout(resolve, step);
      await promise;
      if (owner.cancelled) throw new Cancelled();
      if (onScreen && !document.hidden) left -= step;
    }
  };

  const renderHome = () => {
    const { level, sweep } = overallRisk(posts);
    field('headline').textContent = auditHeadline(posts.length);
    field('banner-cta').textContent = posts.length ? 'review now →' : 'scan again →';
    field('count').textContent = String(posts.length);
    field('risk-label').textContent = riskLabel(level);
    field('risk-sub').textContent = `${posts.length} flagged ${posts.length === 1 ? 'item' : 'items'}`;
    riskCard.dataset.level = level;
    gauge.setAttribute('stroke-dasharray', `${(circumference * sweep) / 360} ${circumference}`);
    flagged.classList.toggle('is-empty', posts.length === 0);
  };

  const showToast = (message: string) => {
    toast.textContent = message;
    toast.classList.add('is-shown');
    clearTimeout(toastTimer);
    toastTimer = window.setTimeout(() => toast.classList.remove('is-shown'), 1900);
  };

  const reset = () => {
    posts = [...DEMO_POSTS];
    current = null;
    scanProgress = 0;
    list.replaceChildren(...pristineRows.map((row) => row.cloneNode(true)));
    renderHome();
  };

  const openDetail = (post: DemoPost) => {
    current = post;
    detail.dataset.risk = post.risk;
    field('d-risk').textContent = `${riskLabel(post.risk)} Risk`;
    field('d-explanation').textContent = post.explanation;
    $<HTMLImageElement>('[data-field="d-icon"]').src = icons[post.platform];
    field('d-platform').textContent = post.platformLabel;
    field('d-date').textContent = post.date;
    field('d-quote').textContent = post.quote;
    field('d-likes').textContent = String(post.likes);
    field('d-comments').textContent = String(post.comments);
    field('d-why').textContent = post.whyFlagged;
    setView('detail');
  };

  const review = (action: ReviewAction) => {
    if (!current) return;
    const resolved = current;
    current = null;
    posts = posts.filter((post) => post !== resolved);
    setView('home');
    window.setTimeout(() => {
      const row = list.querySelector<HTMLElement>(`[data-id="${resolved.id}"]`);
      row?.classList.add('is-leaving');
      window.setTimeout(() => row?.remove(), 450);
      renderHome();
      showToast(TOASTS[action]);
    }, 380);
  };

  // Resumable: a visitor taking over mid-scan finishes the same scan instead of restarting it.
  const runScan = async (owner: Run) => {
    setView('scan');
    while (scanProgress < 1) {
      scanProgress = Math.min(1, Math.round((scanProgress + 0.05) * 100) / 100);
      scanFill.style.width = `${Math.max(5, scanProgress * 100)}%`;
      scanMessage.textContent = SCAN_MESSAGES.findLast(([at]) => scanProgress >= at)?.[1] ?? '';
      scanPlatforms.forEach((el, i) => el.classList.toggle('is-done', scanProgress >= (i + 1) / scanPlatforms.length));
      await sleep(160, owner);
    }
    await sleep(700, owner);
    setView('home');
  };

  const tap = async (target: HTMLElement, owner: Run) => {
    const frame = screen.getBoundingClientRect();
    const box = target.getBoundingClientRect();
    const scale = frame.width / screen.offsetWidth; // CSS zoom on small screens
    const x = (box.left + box.width / 2 - frame.left) / scale;
    const y = (box.top + box.height / 2 - frame.top) / scale;
    finger.classList.add('is-shown');
    finger.style.transform = `translate(${x}px, ${y}px)`;
    await sleep(800, owner);
    finger.classList.remove('is-pressing');
    void finger.offsetWidth;
    finger.classList.add('is-pressing');
    target.classList.add('is-pressed');
    await sleep(200, owner);
    target.classList.remove('is-pressed');
    target.click();
  };

  const autoplay = async (owner: Run) => {
    for (;;) {
      reset();
      finger.style.transform = 'translate(300px, 700px)';
      await runScan(owner);
      await sleep(1500, owner);
      await tap($(`.d-row__btn[data-id="${posts[0].id}"]`), owner);
      await sleep(2800, owner);
      await tap($('[data-action="delete"]'), owner);
      await sleep(2200, owner);
      await tap($('[data-action="review"]'), owner);
      await sleep(2600, owner);
      await tap($('[data-action="keep"]'), owner);
      await sleep(2600, owner);
      finger.classList.remove('is-shown');
      await sleep(900, owner);
    }
  };

  const start = (task: (owner: Run) => Promise<void>) => {
    run.cancelled = true;
    const owner: Run = { cancelled: false };
    run = owner;
    task(owner).catch((error: unknown) => {
      if (!(error instanceof Cancelled)) throw error;
    });
  };

  const takeOver = () => {
    if (root.dataset.mode === 'manual') return;
    root.dataset.mode = 'manual';
    finger.classList.remove('is-shown');
    if (screen.dataset.view === 'scan') start(runScan);
    else run.cancelled = true;
  };

  const rescan = () => {
    reset();
    start(runScan);
  };

  screen.addEventListener('pointerdown', (event) => event.isTrusted && takeOver());
  screen.addEventListener('keydown', (event) => event.isTrusted && takeOver());
  screen.addEventListener('click', (event) => {
    const button = (event.target as Element).closest<HTMLElement>('[data-action]');
    const action = button?.dataset.action;
    if (!action) return;
    if (action === 'open') {
      const post = posts.find(({ id }) => id === button.dataset.id);
      if (post) openDetail(post);
    } else if (action === 'review') {
      if (posts[0]) openDetail(posts[0]);
      else rescan();
    } else if (action === 'back') {
      current = null;
      setView('home');
    } else if (action === 'rescan') {
      rescan();
    } else {
      review(action as ReviewAction);
    }
  });

  root.querySelector<HTMLElement>('[data-replay]')?.addEventListener('click', () => {
    root.dataset.mode = 'auto';
    start(autoplay);
  });

  new IntersectionObserver(([entry]) => {
    onScreen = entry.isIntersecting;
  }, { threshold: 0.15 }).observe(screen);

  setView('scan');
  start(autoplay);
}
