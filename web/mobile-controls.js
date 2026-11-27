// Approved mobile control icons, embedded so the web game needs no icon CDN.
const icons = {
  "door": '<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" aria-hidden="true"><path d="M3 12 9 4h11v16H3Z M4 12h16 M15 15h3"/></svg>',
  "map": "<svg class=\"icon\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><path d=\"m3 6 6-3 6 3 6-3v15l-6 3-6-3-6 3Z M9 3v15 M15 6v15\"/></svg>",
  "phone": "<svg class=\"icon\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><rect x=\"6\" y=\"2.5\" width=\"12\" height=\"19\" rx=\"3\"/><path d=\"M10 6h4 M11 18h2\"/></svg>",
  "menu": "<svg class=\"icon\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><rect x=\"6\" y=\"4\" width=\"4\" height=\"16\" rx=\"1\" fill=\"currentColor\" stroke=\"none\"/><rect x=\"14\" y=\"4\" width=\"4\" height=\"16\" rx=\"1\" fill=\"currentColor\" stroke=\"none\"/></svg>",
  "forward": "<svg class=\"icon\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><path d=\"m6 13 6-6 6 6 M12 7v13\"/></svg>",
  "backward": "<svg class=\"icon\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><path d=\"m6 11 6 6 6-6 M12 17V4\"/></svg>",
  "left": "<svg class=\"icon\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><path d=\"m11 6-6 6 6 6 M5 12h15\"/></svg>",
  "right": "<svg class=\"icon\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><path d=\"m13 6 6 6-6 6 M19 12H4\"/></svg>",
  "run": "<svg class=\"icon\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><circle cx=\"15\" cy=\"4\" r=\"2\" fill=\"currentColor\" stroke=\"none\"/><path d=\"m9 7 4-1 3 5 4 1 M13 7l-3 6 5 3 1 5 M10 13l-3 5H3 M9 7l-3 4H3\"/></svg>",
  "jump": "<svg class=\"icon\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><circle cx=\"12\" cy=\"4\" r=\"2\" fill=\"currentColor\" stroke=\"none\"/><path d=\"m5 5 4 4h6l4-4 M12 9v6 M7 19l5-4 5 4 M4 22h16\"/></svg>",
  "pedal": "<svg class=\"icon\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><path d=\"m10 4 7-1a2 2 0 0 1 2 2l1 10a2 2 0 0 1-2 2l-7 1a2 2 0 0 1-2-2L8 6a2 2 0 0 1 2-2Z M12 7l1 7 M16 6l1 7 M14 18v3H6\"/></svg>",
  "brake": "<svg class=\"icon\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><circle cx=\"12\" cy=\"12\" r=\"7\"/><path d=\"M3 5a11 11 0 0 0 0 14 M21 5a11 11 0 0 1 0 14 M11 16V8h2a2.5 2.5 0 0 1 0 5h-2\"/></svg>",
  "enter": "<svg class=\"icon\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><path d=\"M14 4h6v16h-6 M3 12h12 m-4-4 4 4-4 4\"/></svg>",
  "exit": "<svg class=\"icon\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><path d=\"M10 4H4v16h6 M9 12h12 m-4-4 4 4-4 4\"/></svg>",
  "fullscreen": '<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 4H4v5 m11-5h5v5 M4 15v5h5 m11-5v5h-5"/></svg>',
  "windowed": '<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 9h5V4 m6 0v5h5 M4 15h5v5 m6 0v-5h5"/></svg>'
};

// CSS pixels, independent of device pixel ratio. Desktop-sized web views stay unchanged.
const MOBILE_VIEW = '(max-width: 600px), (max-width: 1024px) and (max-height: 600px)';

export function installMobileControls(api, canvas) {
  const A = api.MobileAction;
  const viewport = matchMedia(MOBILE_VIEW);
  const root = document.createElement('div');
  root.id = 'mobile-controls';
  root.lang = 'fr';
  root.hidden = true;
  const button = (id, icon, label, extra = '') => `<button type="button" id="mobile-${id}" class="mobile-button ${extra}" aria-label="${label}">${icons[icon]}<span>${label}</span></button>`;
  root.innerHTML = `
    <nav class="mobile-utilities" aria-label="Jeu">
      ${button('map', 'map', 'Carte')}
      ${button('phone', 'phone', 'Tél.')}
      ${button('menu', 'menu', 'Menu')}
      ${button('door', 'door', 'Portière')}
      ${button('fullscreen', 'fullscreen', 'Plein écran')}
    </nav>
    <div class="mobile-directions" role="group" aria-label="Déplacement">
      ${button('forward', 'forward', 'Avancer', 'direction up')}
      ${button('left', 'left', 'Tourner à gauche', 'direction left')}
      ${button('backward', 'backward', 'Reculer', 'direction down')}
      ${button('right', 'right', 'Tourner à droite', 'direction right')}
    </div>
    ${button('vehicle', 'enter', 'Monter', 'mobile-vehicle')}
    <div class="mobile-actions" role="group" aria-label="Actions">
      ${button('secondary', 'jump', 'Sauter')}
      ${button('primary', 'run', 'Courir', 'primary')}
    </div>`;
  document.body.append(root);
  const buttons = Object.fromEntries([...root.querySelectorAll('button')].map(b => [b.id.slice(7), b]));
  const holds = new Map();
  let state = 'hidden';
  const holdActions = {
    skiing: { forward: A.Forward, backward: A.Backward, left: A.Left, right: A.Right, secondary: A.Jump, primary: A.Poles },
    walking: { forward: A.Forward, backward: A.Backward, left: A.Left, right: A.Right, secondary: A.Jump, primary: A.Run },
    driving: { forward: A.Forward, backward: A.Backward, left: A.Left, right: A.Right, secondary: A.Jump, primary: A.Forward },
  };
  const available = () => viewport.matches && state !== 'hidden' && !document.hidden;
  const actionHeld = action => [...holds.values()].some(hold => hold.action === action);
  function release(id) {
    const hold = holds.get(id);
    if (!hold) return;
    holds.delete(id);
    if (!actionHeld(hold.action)) api.mobile_input(hold.action, false);
    if (![...holds.values()].some(h => h.button === hold.button)) {
      hold.button.classList.remove('pressed');
      hold.button.setAttribute('aria-pressed', 'false');
    }
  }
  function reset() {
    for (const id of [...holds.keys()]) release(id);
    api.mobile_reset();
  }
  function press(id, key, button) {
    if (!available() || holds.has(id)) return;
    const action = holdActions[state][key];
    const alreadyHeld = actionHeld(action);
    holds.set(id, { action, button });
    button.classList.add('pressed');
    button.setAttribute('aria-pressed', 'true');
    if (!alreadyHeld) api.mobile_input(action, true);
  }
  for (const key of Object.keys(holdActions.walking)) {
    const b = buttons[key];
    b.setAttribute('aria-pressed', 'false');
    b.addEventListener('pointerdown', event => {
      if (event.button !== 0) return;
      event.preventDefault();
      canvas.focus({ preventScroll: true });
      b.setPointerCapture(event.pointerId);
      press(event.pointerId, key, b);
    });
    for (const type of ['pointerup', 'pointercancel', 'lostpointercapture']) {
      b.addEventListener(type, event => release(event.pointerId));
    }
    b.addEventListener('keydown', event => {
      if (event.key !== ' ' && event.key !== 'Enter') return;
      event.preventDefault(); event.stopPropagation();
      press(`key:${key}:${event.key}`, key, b);
    });
    b.addEventListener('keyup', event => {
      if (event.key !== ' ' && event.key !== 'Enter') return;
      event.preventDefault(); event.stopPropagation();
      release(`key:${key}:${event.key}`);
    });
    b.addEventListener('blur', () => {
      for (const id of [...holds.keys()]) if (typeof id === 'string' && holds.get(id).button === b) release(id);
    });
  }
  for (const [key, action] of Object.entries({ map: A.Map, phone: A.Phone, menu: A.Menu, vehicle: A.Vehicle, door: A.Door })) {
    buttons[key].addEventListener('pointerdown', event => event.preventDefault());
    buttons[key].addEventListener('click', () => {
      if (!available()) return;
      reset();
      canvas.focus({ preventScroll: true });
      api.mobile_input(action, true);
    });
  }
  buttons.phone.setAttribute('aria-label', 'Ouvrir le téléphone');
  buttons.phone.title = 'Ouvrir le téléphone';
  root.addEventListener('contextmenu', event => event.preventDefault());
  const setButton = (button, icon, label) => {
    button.innerHTML = `${icons[icon]}<span>${label}</span>`;
    button.setAttribute('aria-label', label);
  };
  const fullscreen = buttons.fullscreen;
  const page = document.documentElement;
  function syncFullscreenButton() {
    const active = document.fullscreenElement === page;
    const label = active ? 'Quitter le plein écran' : 'Plein écran';
    fullscreen.hidden = !document.fullscreenEnabled || typeof page.requestFullscreen !== 'function';
    fullscreen.title = label;
    setButton(fullscreen, active ? 'windowed' : 'fullscreen', label);
  }
  fullscreen.addEventListener('pointerdown', event => event.preventDefault());
  fullscreen.addEventListener('click', async () => {
    if (!available() || fullscreen.disabled) return;
    reset();
    canvas.focus({ preventScroll: true });
    fullscreen.disabled = true;
    try {
      if (document.fullscreenElement === page) {
        await document.exitFullscreen();
        return;
      }
      await page.requestFullscreen();
    } catch {
      // Fullscreen can be denied even when the API is available.
      return;
    } finally {
      fullscreen.disabled = false;
    }
    if (document.fullscreenElement === page) {
      try {
        // The browser releases a fullscreen orientation lock when fullscreen ends.
        await window.screen.orientation?.lock?.('landscape');
      } catch {
        // Keep fullscreen available when the device cannot lock its orientation.
      }
    }
  });
  document.addEventListener('fullscreenchange', syncFullscreenButton);
  syncFullscreenButton();

  function render() {
    const next = canvas.dataset.mobileMode || 'hidden';
    if (next !== state) {
      reset();
      state = next;
      const driving = state === 'driving';
      const skiing = state === 'skiing';
      root.dataset.mode = state;
      setButton(buttons.primary, skiing ? 'forward' : driving ? 'pedal' : 'run', skiing ? 'Bâtons' : driving ? 'Accélérer' : 'Courir');
      setButton(buttons.secondary, skiing || driving ? 'brake' : 'jump', skiing ? 'Freiner' : driving ? 'Frein à main' : 'Sauter');
      setButton(buttons.vehicle, driving ? 'exit' : 'enter', driving ? 'Descendre' : 'Monter');
      buttons.forward.setAttribute('aria-label', skiing ? 'Glisser' : driving ? 'Accélérer' : 'Avancer');
      buttons.backward.setAttribute('aria-label', skiing ? 'Freiner' : driving ? 'Freiner / reculer' : 'Reculer');
    }
    root.hidden = !available();
  }
  new MutationObserver(render).observe(canvas, { attributes: true, attributeFilter: ['data-mobile-mode'] });
  function configure() {
    reset();
    api.mobile_set_enabled(viewport.matches);
    render();
  }
  viewport.addEventListener('change', configure);
  window.addEventListener('resize', configure);
  window.addEventListener('blur', reset);
  window.addEventListener('pagehide', reset);
  document.addEventListener('visibilitychange', () => { reset(); render(); });
  configure();
}
