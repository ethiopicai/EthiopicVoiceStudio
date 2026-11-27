#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>

// Listen-only: no keyboard events are suppressed, recorded, or forwarded as text.
typedef bool (*StudioFnCallback)(bool pressed, bool cancelled);
static StudioFnCallback callback;
static CFMachPortRef tap;
static CFRunLoopSourceRef tapSource;
static bool held, accepted, chord, primaryHeld;
static uint32_t shortcutModifiers;
static int shortcutKey = 63;
static unsigned long generation;
static AXUIElementRef targetField;
static pid_t targetPid;
static NSPanel *panel;
static NSTextField *caption;

static void clearTarget(void) {
    if (targetField) CFRelease(targetField);
    targetField = NULL; targetPid = 0;
}
static void captureTarget(void) {
    clearTarget();
    targetPid = NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier;
    AXUIElementRef app = AXUIElementCreateApplication(targetPid);
    CFTypeRef field = NULL;
    if (AXUIElementCopyAttributeValue(app, kAXFocusedUIElementAttribute, &field) == kAXErrorSuccess &&
        field && CFGetTypeID(field) == AXUIElementGetTypeID()) targetField = (AXUIElementRef)field;
    else if (field) CFRelease(field);
    CFRelease(app);
}
static void overlay(NSString *text) {
    if (!panel) {
        panel = [[NSPanel alloc] initWithContentRect:NSMakeRect(0,0,440,104)
            styleMask:NSWindowStyleMaskBorderless|NSWindowStyleMaskNonactivatingPanel
            backing:NSBackingStoreBuffered defer:NO];
        panel.level = NSFloatingWindowLevel; panel.hidesOnDeactivate = NO;
        panel.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces|NSWindowCollectionBehaviorFullScreenAuxiliary;
        panel.backgroundColor = NSColor.windowBackgroundColor; panel.hasShadow = YES;
        panel.contentView.wantsLayer = YES; panel.contentView.layer.cornerRadius = 16;
        NSTextField *title = [NSTextField labelWithString:@"●  Ethiopic Voice Studio · Dictation"];
        title.frame = NSMakeRect(18,72,405,18); title.textColor = NSColor.systemRedColor;
        title.font = [NSFont systemFontOfSize:12 weight:NSFontWeightSemibold];
        caption = [NSTextField wrappingLabelWithString:@""];
        caption.frame = NSMakeRect(18,12,405,52); caption.font = [NSFont systemFontOfSize:15];
        [panel.contentView addSubview:title]; [panel.contentView addSubview:caption];
    }
    caption.stringValue = text ?: @"Listening…";
    NSRect visible = NSScreen.mainScreen.visibleFrame;
    [panel setFrameOrigin:NSMakePoint(NSMidX(visible)-220,NSMinY(visible)+48)];
    [panel orderFrontRegardless]; // Nonactivating panel preserves the insertion target.
}
static CGEventRef eventCallback(CGEventTapProxy proxy, CGEventType type, CGEventRef event, void *context) {
    (void)proxy; (void)context;
    if (type == kCGEventTapDisabledByTimeout || type == kCGEventTapDisabledByUserInput) {
        if (accepted && callback) callback(false,true);
        held = accepted = primaryHeld = false; generation++;
        if (tap) CGEventTapEnable(tap,true);
        return event;
    }
    int key = (int)CGEventGetIntegerValueField(event,kCGKeyboardEventKeycode);
    CGEventFlags flags = CGEventGetFlags(event);
    uint32_t modifiers = ((flags & kCGEventFlagMaskControl) ? 1 : 0) |
        ((flags & kCGEventFlagMaskAlternate) ? 2 : 0) |
        ((flags & kCGEventFlagMaskShift) ? 4 : 0) |
        ((flags & kCGEventFlagMaskCommand) ? 8 : 0);
    if (shortcutKey == 63 && type == kCGEventFlagsChanged && key == 63)
        primaryHeld = (flags & kCGEventFlagMaskSecondaryFn) != 0;
    else if (shortcutKey >= 0 && key == shortcutKey && type == kCGEventKeyDown) primaryHeld = true;
    else if (shortcutKey >= 0 && key == shortcutKey && type == kCGEventKeyUp) primaryHeld = false;
    if (chord) {
        if ((shortcutKey >= 0 && primaryHeld) || (shortcutKey < 0 && (modifiers & shortcutModifiers))) return event;
        chord = false;
    }
    if (type == kCGEventKeyDown && held && key != shortcutKey) {
        chord = true; generation++;
        if (accepted && callback) callback(false,true);
        accepted = false;
        return event;
    }
    bool next = modifiers == shortcutModifiers && (shortcutKey < 0 || primaryHeld);
    if (next == held) return event;
    held = next; unsigned long token = ++generation;
    if (held) {
        chord = false;
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW,200*NSEC_PER_MSEC),dispatch_get_main_queue(),^{
            if (!held || chord || token != generation || !callback) return;
            if (callback(true,false)) { accepted = true; captureTarget(); overlay(@"Getting ready…"); }
        });
    } else if (accepted) {
        accepted = false;
        bool unexpectedModifier = (modifiers & ~shortcutModifiers) != 0;
        if (unexpectedModifier) chord = true;
        if (callback) callback(false,unexpectedModifier);
    }
    return event;
}
void studio_fn_set_binding(uint32_t modifiers, int key) {
    generation++; held = accepted = chord = primaryHeld = false;
    shortcutModifiers = modifiers; shortcutKey = key;
}
bool studio_fn_input_allowed(void) { return CGPreflightListenEventAccess(); }
bool studio_fn_accessibility_allowed(void) { return AXIsProcessTrusted(); }
void studio_fn_request_permissions(void) {
    CGRequestListenEventAccess();
    NSDictionary *options = @{(__bridge NSString *)kAXTrustedCheckOptionPrompt:@YES};
    AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)options);
}
bool studio_fn_enable(StudioFnCallback handler) {
    callback = handler;
    if (tap) return true;
    if (!studio_fn_input_allowed() || !studio_fn_accessibility_allowed()) return false;
    tap = CGEventTapCreate(kCGSessionEventTap,kCGHeadInsertEventTap,kCGEventTapOptionListenOnly,
        CGEventMaskBit(kCGEventFlagsChanged)|CGEventMaskBit(kCGEventKeyDown)|CGEventMaskBit(kCGEventKeyUp),eventCallback,NULL);
    if (!tap) return false;
    tapSource = CFMachPortCreateRunLoopSource(kCFAllocatorDefault,tap,0);
    CFRunLoopAddSource(CFRunLoopGetMain(),tapSource,kCFRunLoopCommonModes);
    CGEventTapEnable(tap,true); return true;
}
void studio_fn_disable(void) {
    generation++; held = accepted = primaryHeld = false;
    if (tap) { CGEventTapEnable(tap,false); CFMachPortInvalidate(tap); CFRelease(tap); tap = NULL; }
    if (tapSource) { CFRunLoopRemoveSource(CFRunLoopGetMain(),tapSource,kCFRunLoopCommonModes); CFRelease(tapSource); tapSource = NULL; }
    clearTarget(); [panel orderOut:nil];
}
void studio_fn_overlay(const char *text) {
    NSString *value = text ? [NSString stringWithUTF8String:text] : @"Listening…";
    dispatch_async(dispatch_get_main_queue(),^{ overlay(value); });
}
void studio_fn_clear(void) { dispatch_async(dispatch_get_main_queue(),^{ clearTarget(); [panel orderOut:nil]; }); }
// Return: 0 inserted, 1 focus changed/unavailable, 2 permission missing, 3 secure field,
// 4 insertion unavailable. Prefer direct Accessibility insertion; apps that do
// not support it receive a targeted Paste event using only the generated text.
int studio_fn_insert(const char *text) {
    if (!AXIsProcessTrusted()) return 2;
    if (!targetField || targetPid == getpid() || NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier != targetPid) return 1;
    AXUIElementRef app = AXUIElementCreateApplication(targetPid);
    CFTypeRef field = NULL, subrole = NULL, role = NULL;
    AXUIElementCopyAttributeValue(app,kAXFocusedUIElementAttribute,&field); CFRelease(app);
    bool matches = field && CFEqual(field,targetField);
    if (field) CFRelease(field);
    if (!matches) return 1;
    AXUIElementCopyAttributeValue(targetField,kAXSubroleAttribute,&subrole);
    bool secure = subrole && CFEqual(subrole,CFSTR("AXSecureTextField"));
    if (subrole) CFRelease(subrole);
    if (secure) return 3;
    AXUIElementCopyAttributeValue(targetField,kAXRoleAttribute,&role);
    bool textEntry = role && (CFEqual(role,kAXTextFieldRole) || CFEqual(role,kAXTextAreaRole) || CFEqual(role,kAXComboBoxRole));
    if (role) CFRelease(role);
    if (!textEntry) return 4;
    Boolean settable = false;
    NSString *value = [NSString stringWithUTF8String:text];
    if (!value.length) return 4;
    if (AXUIElementIsAttributeSettable(targetField,kAXSelectedTextAttribute,&settable) == kAXErrorSuccess && settable &&
        AXUIElementSetAttributeValue(targetField,kAXSelectedTextAttribute,(__bridge CFStringRef)value) == kAXErrorSuccess) return 0;
    // No clipboard reads or backups. Keep generated text off Universal Clipboard.
    CGEventRef down = CGEventCreateKeyboardEvent(NULL,9,true);
    CGEventRef up = CGEventCreateKeyboardEvent(NULL,9,false);
    if (!down || !up) { if(down)CFRelease(down); if(up)CFRelease(up); return 4; }
    NSPasteboard *board = NSPasteboard.generalPasteboard;
    [board prepareForNewContentsWithOptions:NSPasteboardContentsCurrentHostOnly];
    if (![board setString:value forType:NSPasteboardTypeString]) { CFRelease(down); CFRelease(up); return 4; }
    CGEventSetFlags(down,kCGEventFlagMaskCommand); CGEventSetFlags(up,kCGEventFlagMaskCommand);
    CGEventPostToPid(targetPid,down); CGEventPostToPid(targetPid,up);
    CFRelease(down); CFRelease(up); return 0;
}
