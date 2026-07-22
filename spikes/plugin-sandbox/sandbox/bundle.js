// A trivial "plugin": the framework-agnostic mount contract from the plan.
export default {
  mount(rootEl, _api, _context) {
    rootEl.textContent = 'plugin mounted in sandbox';
    return { mounted: true };
  },
};
