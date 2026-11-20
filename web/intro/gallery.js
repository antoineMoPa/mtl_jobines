(() => {
  const gallery = document.getElementById('district-views');
  const previous = document.getElementById('district-prev');
  const next = document.getElementById('district-next');
  const pictures = Array.from(gallery.children);

  function updateControls() {
    previous.disabled = gallery.scrollLeft <= 1;
    next.disabled = gallery.scrollLeft + gallery.clientWidth >= gallery.scrollWidth - 1;
  }

  previous.addEventListener('click', () => {
    const target = pictures.findLast(picture => picture.offsetLeft < gallery.scrollLeft - 1);
    gallery.scrollTo({ left: target?.offsetLeft ?? 0 });
  });
  next.addEventListener('click', () => {
    const target = pictures.find(picture => picture.offsetLeft > gallery.scrollLeft + 1);
    gallery.scrollTo({ left: target?.offsetLeft ?? gallery.scrollWidth });
  });
  gallery.addEventListener('scroll', updateControls, { passive: true });
  new ResizeObserver(updateControls).observe(gallery);
  previous.parentElement.hidden = false;
  updateControls();
})();
