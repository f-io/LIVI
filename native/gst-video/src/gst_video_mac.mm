#include <gst/gst.h>
#import <Cocoa/Cocoa.h>
#import <QuartzCore/QuartzCore.h>
#include <math.h>

// Clip view: sized to the AA content rectangle (the user-chosen AR the phone renders
// inside the 16:9 transport tier), centered in the window, clipping its child
@interface LIVIClipView : NSView {
@public
  NSView* _gl;  // the GL sink's render target (child view)
  double _cropL, _cropT, _visW, _visH, _tierW, _tierH;  // content region in tier px
@private
  BOOL _relayoutPending;  // coalesce deferred relayouts during a live window resize
  BOOL _inLiveResize;     // window is in an interactive drag-resize (plane suspended)
  BOOL _userHidden;       // logical visibility from livi_set_view_hidden (restored after resize)
}
- (void)relayout;
- (void)setUserHidden:(BOOL)hidden;
@end

@implementation LIVIClipView
- (NSView*)hitTest:(NSPoint)point {
  return nil;
}

- (void)relayout {
  NSView* sv = [self superview];
  if (!sv) return;
  const double ww = sv.bounds.size.width;
  const double wh = sv.bounds.size.height;
  if (ww <= 0 || wh <= 0) return;

  // No content region yet: fill the window, child fills the clip view.
  if (_visW <= 0 || _visH <= 0 || _tierW <= 0 || _tierH <= 0) {
    [self setFrame:sv.bounds];
    [_gl setFrame:self.bounds];
    return;
  }

  // Contain the content AR into the window; the clip view IS that content rect.
  const double scale = fmin(ww / _visW, wh / _visH);
  const double cdw = _visW * scale;
  const double cdh = _visH * scale;
  [self setFrame:NSMakeRect((ww - cdw) / 2.0, (wh - cdh) / 2.0, cdw, cdh)];

  // Child = whole tier scaled by `scale`, shifted so the content (at cropL/cropT inside
  // the tier) sits at the clip origin. The clip view bounds clip the margins
  [_gl setFrame:NSMakeRect(-_cropL * scale, -_cropT * scale, _tierW * scale, _tierH * scale)];
}

- (void)superviewResized:(NSNotification*)note {
  (void)note;
  // While the window is in an interactive live resize the plane is hidden and we relayout once at
  // the end (windowDidEndLiveResize:), so skip the per-frame churn entirely here.
  if (_inLiveResize) return;
  // Defer + coalesce. Changing our frame synchronously inside AppKit's live window resize trips
  // the NSView visible-rect cache assertion (-[NSView setFrameSize:] ->
  // NSViewHierarchyInvalidateVisibleRect), which aborts on drag-resize. Running the relayout on
  // the next main-loop turn keeps it out of AppKit's in-progress resize pass.
  if (_relayoutPending) return;
  _relayoutPending = YES;
  dispatch_async(dispatch_get_main_queue(), ^{
    self->_relayoutPending = NO;
    [self relayout];
  });
}

// Logical visibility (cluster shown/hidden)
- (void)setUserHidden:(BOOL)hidden {
  _userHidden = hidden;
  if (!_inLiveResize) [self setHidden:hidden];
}

- (void)windowWillStartLiveResize:(NSNotification*)note {
  (void)note;
  _inLiveResize = YES;
  [self setHidden:YES];
}

- (void)windowDidEndLiveResize:(NSNotification*)note {
  (void)note;
  _inLiveResize = NO;
  [self setHidden:_userHidden];
  [self relayout];
}
@end

extern "C" guintptr livi_attach_view(guintptr parent, void** outView) {
  *outView = nullptr;
  NSView* p = (NSView*)(void*)parent;
  if (!p) return parent;

  LIVIClipView* clip = [[LIVIClipView alloc] initWithFrame:[p bounds]];
  clip->_gl = nullptr;
  clip->_cropL = clip->_cropT = clip->_visW = clip->_visH = clip->_tierW = clip->_tierH = 0;
  [clip setWantsLayer:YES];
  clip.layer.backgroundColor = CGColorGetConstantColor(kCGColorBlack);
  clip.layer.masksToBounds = YES;
  [p addSubview:clip positioned:NSWindowBelow relativeTo:nil];

  NSView* gl = [[NSView alloc] initWithFrame:[clip bounds]];
  [gl setWantsLayer:YES];
  [clip addSubview:gl];
  clip->_gl = gl;

  // Re-lay-out whenever the window (content view) resizes.
  [p setPostsFrameChangedNotifications:YES];
  [[NSNotificationCenter defaultCenter] addObserver:clip
                                           selector:@selector(superviewResized:)
                                               name:NSViewFrameDidChangeNotification
                                             object:p];

  // Suspend the plane during an interactive window drag-resize (see windowWillStartLiveResize:).
  NSWindow* win = [p window];
  if (win) {
    NSNotificationCenter* nc = [NSNotificationCenter defaultCenter];
    [nc addObserver:clip
           selector:@selector(windowWillStartLiveResize:)
               name:NSWindowWillStartLiveResizeNotification
             object:win];
    [nc addObserver:clip
           selector:@selector(windowDidEndLiveResize:)
               name:NSWindowDidEndLiveResizeNotification
             object:win];
  }

  *outView = (void*)clip;       // tracked view; region/hide/remove operate on the clip
  return (guintptr)(void*)gl;   // the GL sink renders into the child
}

// Set the AA content region (crop offsets + visible size within the decoded tier) and
// re-lay-out. cropL=0/visW=0 disables cropping (child fills the window)
extern "C" void livi_set_content_region(void* view, void* sink, double cropL,
    double cropT, double visW, double visH, double tierW, double tierH) {
  (void)sink;
  if (!view) return;
  LIVIClipView* clip = (LIVIClipView*)view;
  clip->_cropL = cropL;
  clip->_cropT = cropT;
  clip->_visW = visW;
  clip->_visH = visH;
  clip->_tierW = tierW;
  clip->_tierH = tierH;
  [clip relayout];
}

extern "C" void livi_remove_view(void* view) {
  if (!view) return;
  NSView* v = (NSView*)view;
  [[NSNotificationCenter defaultCenter] removeObserver:v];
  [v removeFromSuperview];
}

extern "C" void livi_set_view_hidden(void* view, bool hidden) {
  if (!view) return;
  NSView* v = (NSView*)view;
  if ([v isKindOfClass:[LIVIClipView class]]) {
    [(LIVIClipView*)v setUserHidden:hidden];
    return;
  }
  [v setHidden:hidden];
}

extern "C" void livi_set_backdrop(guintptr parent, double r, double g, double b) {
  NSView* p = (NSView*)(void*)parent;
  if (!p) return;
  [p setWantsLayer:YES];
  if (!p.layer) return;
  NSColor* col = [NSColor colorWithSRGBRed:r green:g blue:b alpha:1.0];
  p.layer.backgroundColor = col.CGColor;
}
