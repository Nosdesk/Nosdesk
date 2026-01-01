import { ref, onMounted, onBeforeUnmount, type Ref, type ComputedRef } from 'vue'

// Min/max heights constants
const MIN_SECTION_HEIGHT = 60;
const MIN_OTHER_SECTION_HEIGHT = 60;
const RESIZER_HEIGHT = 8;

// Debug logging - set to false to disable logging
const DEBUG = false;
const logDebug = (...args: unknown[]) => {
  if (DEBUG) console.log('[ResizableSidebar]', ...args);
};

export function useResizableSidebar(
  navbarRef: Ref<HTMLElement | null>,
  ticketsSectionRef: Ref<HTMLElement | null> | ComputedRef<HTMLElement | null>,
  docsSectionRef: Ref<HTMLElement | null> | ComputedRef<HTMLElement | null>,
  resizerRef: Ref<HTMLElement | null>
) {
  const ticketsHeight = ref(200);
  const isResizing = ref(false);
  let dragStartY = 0;
  let initialHeight = 0;
  let rafId: number | null = null;
  
  // Load initial state
  onMounted(() => {
    const storedHeight = localStorage.getItem('ticketsHeight');
    if (storedHeight) {
      ticketsHeight.value = parseInt(storedHeight, 10);
    }
  });
  
  // Apply changes with hardware acceleration
  const applyResize = (newHeight: number) => {
    if (!ticketsSectionRef.value) return;
    
    // Update both the reactive state and directly set the style for immediate feedback
    ticketsHeight.value = newHeight;
    ticketsSectionRef.value.style.maxHeight = `${newHeight}px`;
  };
  
  // Start resize operation
  const startResize = (event: MouseEvent | TouchEvent) => {
    event.preventDefault();
    
    if (!ticketsSectionRef.value) return;
    
    // Enable hardware acceleration
    if (ticketsSectionRef.value) {
      ticketsSectionRef.value.style.willChange = 'max-height';
    }
    
    // Get current height directly from the DOM for accuracy
    initialHeight = ticketsSectionRef.value.offsetHeight;
    
    // Get cursor position
    const clientY = 'touches' in event ? event.touches[0].clientY : event.clientY;
    dragStartY = clientY;
    
    // Set resizing state
    isResizing.value = true;
    
    // Add event listeners with capture phase for immediate response
    document.addEventListener('mousemove', handleResize, { passive: false, capture: true });
    document.addEventListener('mouseup', stopResize, { capture: true });
    document.addEventListener('touchmove', handleResize, { passive: false, capture: true });
    document.addEventListener('touchend', stopResize, { capture: true });
    document.addEventListener('touchcancel', stopResize, { capture: true });
    
    document.body.classList.add('resize-active');
  };
  
  // Handle resize movement with requestAnimationFrame for smooth performance
  const handleResize = (event: MouseEvent | TouchEvent) => {
    if (!isResizing.value) return;
    
    event.preventDefault();
    
    // Cancel any pending animation frame
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
    }
    
    // Use requestAnimationFrame to optimize visual updates
    rafId = requestAnimationFrame(() => {
      if (!navbarRef.value || !ticketsSectionRef.value) return;
      
      // Get current cursor position
      const clientY = 'touches' in event ? event.touches[0].clientY : event.clientY;
      
      // Calculate delta from initial position
      const deltaY = clientY - dragStartY;
      
      // Calculate new height
      let newHeight = initialHeight + deltaY;
      
      // Apply constraints
      const totalHeight = navbarRef.value.offsetHeight;
      const maxHeight = totalHeight - MIN_OTHER_SECTION_HEIGHT - RESIZER_HEIGHT;
      
      // Constrain within min/max bounds
      newHeight = Math.max(MIN_SECTION_HEIGHT, Math.min(newHeight, maxHeight));
      
      // Apply the new height
      applyResize(newHeight);
    });
  };
  
  // Stop resize operation
  const stopResize = () => {
    if (!isResizing.value) return;
    
    // Cancel any pending animation frame
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
    
    isResizing.value = false;
    
    // Disable hardware acceleration hints
    if (ticketsSectionRef.value) {
      ticketsSectionRef.value.style.willChange = 'auto';
    }
    
    // Clean up listeners - make sure to use same options when removing
    document.removeEventListener('mousemove', handleResize, { capture: true });
    document.removeEventListener('mouseup', stopResize, { capture: true });
    document.removeEventListener('touchmove', handleResize, { capture: true });
    document.removeEventListener('touchend', stopResize, { capture: true });
    document.removeEventListener('touchcancel', stopResize, { capture: true });
    document.body.classList.remove('resize-active');
    
    // Save preference
    localStorage.setItem('ticketsHeight', ticketsHeight.value.toString());
  };
  
  // Utility to equalize heights
  const equalizeHeights = () => {
    if (!navbarRef.value || !ticketsSectionRef.value) return;
    
    const navbarRect = navbarRef.value.getBoundingClientRect();
    const totalHeight = navbarRect.height;
    
    // Calculate equal distribution
    const equalHeight = Math.floor((totalHeight - RESIZER_HEIGHT) / 2);
    const finalHeight = Math.max(MIN_SECTION_HEIGHT, equalHeight);
    
    // Apply the height
    applyResize(finalHeight);
    localStorage.setItem('ticketsHeight', finalHeight.toString());
  };
  
  // Cleanup
  onBeforeUnmount(() => {
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
    }
    stopResize();
  });
  
  return {
    ticketsHeight,
    isResizing,
    startResize,
    equalizeHeights
  };
} 