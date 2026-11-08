// Synthetic IME using the real compositor input-method-v2/text-input-v3 bridge.
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/mman.h>
#include <xkbcommon/xkbcommon.h>
#include <xkbcommon/xkbcommon-keysyms.h>
#include <wayland-client.h>
#include "input-method.h"
static struct zwp_input_method_manager_v2 *manager;
static struct wl_seat *seat;
static unsigned serial, key_count;
static struct zwp_input_method_v2 *method;
static int active;
static struct zwp_input_method_keyboard_grab_v2 *grab;
static struct xkb_context *context;
static struct xkb_keymap *map;
static struct xkb_state *keyboard;
static void keymap(void*d,struct zwp_input_method_keyboard_grab_v2*g,uint32_t f,int32_t fd,uint32_t size){
    char *text=mmap(NULL,size,PROT_READ,MAP_PRIVATE,fd,0);close(fd);
    if(text==MAP_FAILED)return;
    if(keyboard)xkb_state_unref(keyboard);
    if(map)xkb_keymap_unref(map);
    map=xkb_keymap_new_from_string(context,text,XKB_KEYMAP_FORMAT_TEXT_V1,XKB_KEYMAP_COMPILE_NO_FLAGS);
    keyboard=map?xkb_state_new(map):NULL;munmap(text,size);
}
static void key(void*d,struct zwp_input_method_keyboard_grab_v2*g,uint32_t key_serial,uint32_t time,uint32_t key,uint32_t state){
    key_count++;
    if(keyboard && xkb_state_key_get_one_sym(keyboard,key+8)==XKB_KEY_Return && state==WL_KEYBOARD_KEY_STATE_PRESSED){
        zwp_input_method_v2_set_preedit_string(method,"",0,0);
        zwp_input_method_v2_commit_string(method,"中文");
        zwp_input_method_v2_commit(method,serial);
        zwp_input_method_keyboard_grab_v2_release(grab);grab=NULL;
    }
}
static void modifiers(void*d,struct zwp_input_method_keyboard_grab_v2*g,uint32_t serial,uint32_t depressed,uint32_t latched,uint32_t locked,uint32_t group){if(keyboard)xkb_state_update_mask(keyboard,depressed,latched,locked,0,0,group);}
static void repeat(void*d,struct zwp_input_method_keyboard_grab_v2*g,int32_t rate,int32_t delay){}
static const struct zwp_input_method_keyboard_grab_v2_listener keys={keymap,key,modifiers,repeat};
static void global(void*d,struct wl_registry*r,uint32_t n,const char*i,uint32_t v) {
    if(!strcmp(i,"zwp_input_method_manager_v2"))manager=wl_registry_bind(r,n,&zwp_input_method_manager_v2_interface,1);
    if(!strcmp(i,"wl_seat")&&!seat)seat=wl_registry_bind(r,n,&wl_seat_interface,1);
}
static void removed(void*d,struct wl_registry*r,uint32_t n){}
static void activate(void*d,struct zwp_input_method_v2*i){active=1;}
static void deactivate(void*d,struct zwp_input_method_v2*i){active=0;}
static void surrounding(void*d,struct zwp_input_method_v2*i,const char*t,uint32_t c,uint32_t a){}
static void cause(void*d,struct zwp_input_method_v2*i,uint32_t c){}
static void content(void*d,struct zwp_input_method_v2*i,uint32_t h,uint32_t p){}
static void done(void*d,struct zwp_input_method_v2*i){serial++;}
static void unavailable(void*d,struct zwp_input_method_v2*i){fprintf(stderr,"IME unavailable\n");exit(5);}
int main(void) {
    context=xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    struct wl_display *display=wl_display_connect(NULL);if(!display)return 2;
    struct wl_registry *registry=wl_display_get_registry(display);
    const struct wl_registry_listener globals={global,removed};
    wl_registry_add_listener(registry,&globals,NULL);wl_display_roundtrip(display);
    if(!manager||!seat)return 3;
    struct zwp_input_method_v2 *im=zwp_input_method_manager_v2_get_input_method(manager,seat);
    method=im;
    const struct zwp_input_method_v2_listener listener={activate,deactivate,surrounding,cause,content,done,unavailable};
    zwp_input_method_v2_add_listener(im,&listener,NULL);wl_display_roundtrip(display);
    char line[4096];
    for(;;) {
        wl_display_dispatch_pending(display);wl_display_flush(display);
        struct pollfd fds[2]={{0,POLLIN,0},{wl_display_get_fd(display),POLLIN,0}};
        if(poll(fds,2,-1)<0)break;
        if(fds[1].revents&POLLIN)if(wl_display_dispatch(display)<0)break;
        if(fds[0].revents&(POLLIN|POLLHUP)) {
            if(!fgets(line,sizeof line,stdin))break;
            line[strcspn(line,"\n")]=0;
            wl_display_roundtrip(display);
            if(line[0]=='p') {
                if(!grab){grab=zwp_input_method_v2_grab_keyboard(im);zwp_input_method_keyboard_grab_v2_add_listener(grab,&keys,NULL);}
                zwp_input_method_v2_set_preedit_string(im,line+2,strlen(line+2),strlen(line+2));
            }
            if(line[0]=='c') {
                zwp_input_method_v2_set_preedit_string(im,"",0,0);
                zwp_input_method_v2_commit_string(im,line+2);
            }
            if(line[0]=='x')zwp_input_method_v2_set_preedit_string(im,"",0,0);
            if(line[0]!='s')zwp_input_method_v2_commit(im,serial);
            wl_display_roundtrip(display);
            if((line[0]=='c'||line[0]=='x')&&grab){zwp_input_method_keyboard_grab_v2_release(grab);grab=NULL;wl_display_roundtrip(display);}
            printf("{\"active\":%d,\"serial\":%u,\"keys\":%u}\n",active,serial,key_count);fflush(stdout);
        }
    }
    zwp_input_method_v2_destroy(im);wl_display_flush(display);wl_display_disconnect(display);return 0;
}
