const nav = document.getElementById('nav');

// Add scrolled style to nav
window.addEventListener('scroll', () => {
  nav.classList.toggle('scrolled', window.scrollY > 10);
}, { passive: true });

// Highlight active nav link based on what is in the middle of the screen:
// the sections, and the device lists inside the Control Center section —
// those win while they are there, so "Devices" lights up, not "Control
// Center".
const sections = document.querySelectorAll('section[id], #device-lists');
const navLinks = document.querySelectorAll('.nav-links a[href^="#"]');
const inView = new Set();

const observer = new IntersectionObserver(entries => {
  entries.forEach(entry => {
    if (entry.isIntersecting) inView.add(entry.target.id);
    else inView.delete(entry.target.id);
  });
  const id = inView.has('device-lists') ? 'device-lists' : [...inView].pop();
  if (!id) return;
  navLinks.forEach(link => {
    link.classList.toggle('active', link.getAttribute('href') === `#${id}`);
  });
}, { rootMargin: '-40% 0px -55% 0px' });

sections.forEach(s => observer.observe(s));

// Settings tab image switcher
document.querySelectorAll('.settings-tab').forEach(tab => {
  tab.addEventListener('click', () => {
    document.querySelectorAll('.settings-tab').forEach(t => {
      t.classList.remove('active');
      t.setAttribute('aria-selected', 'false');
    });
    tab.classList.add('active');
    tab.setAttribute('aria-selected', 'true');
    const img = document.getElementById('settings-preview');
    img.src = 'images/' + tab.dataset.img;
    img.alt = 'Settings — ' + tab.textContent + ' tab';
  });
});

// Supported-device lists: one list at a time behind tabs, a search over all
// three (by name, USB id, type or notes) with the matches counted on each tab.
// Links to #supported-devices, #battery-devices or #settings-devices open
// their tab. Without JavaScript the lists simply stay one after another.
const deviceLists = document.getElementById('device-lists');
if (deviceLists) {
  deviceLists.classList.add('tabbed');
  const tabs = [...deviceLists.querySelectorAll('[data-list]')];
  const panels = tabs.map(t => document.getElementById(t.dataset.list));
  const search = document.getElementById('device-search');
  const empty = deviceLists.querySelector('.device-search-empty');
  const ids = tabs.map(t => t.dataset.list);

  const show = id => tabs.forEach((t, i) => {
    const on = t.dataset.list === id;
    t.classList.toggle('active', on);
    t.setAttribute('aria-selected', on ? 'true' : 'false');
    panels[i].hidden = !on;
  });

  const filter = () => {
    const q = search.value.trim().toLowerCase();
    let inActive = 0;
    tabs.forEach((t, i) => {
      let n = 0;
      panels[i].querySelectorAll('tbody tr').forEach(row => {
        const hit = !q || row.textContent.toLowerCase().includes(q);
        row.hidden = !hit;
        if (hit) n++;
      });
      t.textContent = `${t.dataset.label} (${n})`;
      if (t.classList.contains('active')) inActive = n;
    });
    empty.hidden = inActive > 0;
  };

  // A jump, not a smooth scroll: scrolling smoothly past the Control Center
  // section loads its lazy screenshots on the way, the page grows and the
  // scroll stopped short, in the Control Center. Jumped once more when the
  // page has settled, in case something above still grew. 'instant':
  // the page's CSS makes every scroll smooth (scroll-behavior).
  const open = (id, scroll) => {
    show(id);
    filter();
    if (!scroll) return;
    deviceLists.scrollIntoView({ behavior: 'instant', block: 'start' });
    setTimeout(() => deviceLists.scrollIntoView({ behavior: 'instant', block: 'start' }), 400);
  };

  tabs.forEach(t => t.addEventListener('click', () => {
    open(t.dataset.list, false);
    history.replaceState(null, '', '#' + t.dataset.list);
  }));
  search.addEventListener('input', filter);
  ids.forEach(id => document.querySelectorAll(`a[href="#${id}"]`).forEach(a =>
    a.addEventListener('click', e => {
      e.preventDefault();
      open(id, true);
      history.replaceState(null, '', '#' + id);
    })));
  window.addEventListener('hashchange', () => {
    const id = location.hash.slice(1);
    if (ids.includes(id)) open(id, true);
  });

  const start = location.hash.slice(1);
  open(ids.includes(start) ? start : ids[0], ids.includes(start));
}

// Every other link within the page (the nav bar: Panels, Dialogs, Download,
// Support …) jumps too. The page's CSS scrolls smoothly, and a smooth scroll
// past the Control Center's lazy screenshots loaded them on the way: the
// page grew and the scroll stopped about 2000 px short of anything below
// them (measured in Brave for #panels, #dialogs, #download and #support).
// A jump passes nothing, so nothing loads on the way; jumped once more when
// the page has settled.
document.querySelectorAll('a[href^="#"]').forEach(a => {
  const id = a.getAttribute('href').slice(1);
  const target = id && document.getElementById(id);
  if (!target || a.dataset.list || (deviceLists && ['supported-devices', 'battery-devices', 'settings-devices'].includes(id))) return;
  a.addEventListener('click', e => {
    e.preventDefault();
    target.scrollIntoView({ behavior: 'instant', block: 'start' });
    setTimeout(() => target.scrollIntoView({ behavior: 'instant', block: 'start' }), 400);
    history.replaceState(null, '', '#' + id);
  });
});
