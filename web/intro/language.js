(() => {
  const url = new URL(location.href);
  const link = document.getElementById('language');
  if (url.searchParams.get('lang') === 'en') {
    document.documentElement.lang = 'en';
    for (const element of document.querySelectorAll('[data-en]')) {
      element.textContent = element.dataset.en;
    }
    const attributes = { 'data-en-alt': 'alt', 'data-en-label': 'aria-label', 'data-en-content': 'content' };
    for (const [source, target] of Object.entries(attributes)) {
      for (const element of document.querySelectorAll('[' + source + ']')) {
        element.setAttribute(target, element.getAttribute(source));
      }
    }
    link.textContent = 'Français';
    link.lang = 'fr-CA';
    link.hreflang = 'fr-CA';
    url.searchParams.delete('lang');
  } else {
    url.searchParams.set('lang', 'en');
  }
  link.href = url.href;
  link.hidden = false;
})();
