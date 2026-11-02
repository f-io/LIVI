// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use core::ffi::c_void;
use core::ptr;

type EglDisplay = *mut c_void;
type EglConfig = *mut c_void;
type EglContext = *mut c_void;
type EglSurface = *mut c_void;

const EGL_NONE: i32 = 0x3038;
const EGL_RED_SIZE: i32 = 0x3024;
const EGL_GREEN_SIZE: i32 = 0x3023;
const EGL_BLUE_SIZE: i32 = 0x3022;
const EGL_SURFACE_TYPE: i32 = 0x3033;
const EGL_WINDOW_BIT: i32 = 0x0004;
const EGL_RENDERABLE_TYPE: i32 = 0x3040;
const EGL_OPENGL_ES2_BIT: i32 = 0x0004;
const EGL_CONTEXT_CLIENT_VERSION: i32 = 0x3098;
const GL_COLOR_BUFFER_BIT: u32 = 0x4000;

#[link(name = "EGL")]
unsafe extern "C" {
    fn eglGetDisplay(native: *mut c_void) -> EglDisplay;
    fn eglInitialize(display: EglDisplay, major: *mut i32, minor: *mut i32) -> u32;
    fn eglChooseConfig(
        display: EglDisplay,
        attribs: *const i32,
        configs: *mut EglConfig,
        size: i32,
        count: *mut i32,
    ) -> u32;
    fn eglCreateContext(
        display: EglDisplay,
        config: EglConfig,
        share: EglContext,
        attribs: *const i32,
    ) -> EglContext;
    fn eglCreateWindowSurface(
        display: EglDisplay,
        config: EglConfig,
        window: *mut c_void,
        attribs: *const i32,
    ) -> EglSurface;
    fn eglMakeCurrent(
        display: EglDisplay,
        draw: EglSurface,
        read: EglSurface,
        context: EglContext,
    ) -> u32;
    fn eglSwapBuffers(display: EglDisplay, surface: EglSurface) -> u32;
    fn eglDestroySurface(display: EglDisplay, surface: EglSurface) -> u32;
    fn eglDestroyContext(display: EglDisplay, context: EglContext) -> u32;
}

#[link(name = "GLESv2")]
unsafe extern "C" {
    fn glClearColor(r: f32, g: f32, b: f32, a: f32);
    fn glClear(mask: u32);
}

pub fn paint(window: usize) {
    unsafe {
        let display = eglGetDisplay(ptr::null_mut());
        if display.is_null() || eglInitialize(display, ptr::null_mut(), ptr::null_mut()) == 0 {
            return;
        }
        let attribs = [
            EGL_SURFACE_TYPE,
            EGL_WINDOW_BIT,
            EGL_RENDERABLE_TYPE,
            EGL_OPENGL_ES2_BIT,
            EGL_RED_SIZE,
            8,
            EGL_GREEN_SIZE,
            8,
            EGL_BLUE_SIZE,
            8,
            EGL_NONE,
        ];
        let mut config = ptr::null_mut();
        let mut count = 0;
        if eglChooseConfig(display, attribs.as_ptr(), &mut config, 1, &mut count) == 0 || count < 1
        {
            return;
        }
        let version = [EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE];
        let context = eglCreateContext(display, config, ptr::null_mut(), version.as_ptr());
        if context.is_null() {
            return;
        }
        let surface = eglCreateWindowSurface(display, config, window as *mut c_void, ptr::null());
        if !surface.is_null() {
            if eglMakeCurrent(display, surface, surface, context) != 0 {
                glClearColor(0.0, 0.0, 0.0, 1.0);
                glClear(GL_COLOR_BUFFER_BIT);
                eglSwapBuffers(display, surface);
                eglMakeCurrent(display, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            }
            eglDestroySurface(display, surface);
        }
        eglDestroyContext(display, context);
    }
}
