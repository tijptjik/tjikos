/* Native compositor input for the isolated showcase seat. No Qt test events. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <wayland-client.h>
#include "pointer.h"
static struct wl_output *output;
static struct zwlr_virtual_pointer_manager_v1 *manager;
static const char *wanted;
static void geometry(void*d,struct wl_output*o,int32_t x,int32_t y,int32_t a,int32_t b,int32_t s,const char*m,const char*n,int32_t t){}
static void mode(void*d,struct wl_output*o,uint32_t f,int32_t w,int32_t h,int32_t r){}
static void done(void*d,struct wl_output*o){}
static void scale(void*d,struct wl_output*o,int32_t s){}
static void name(void*d,struct wl_output*o,const char*n){if(!strcmp(n,wanted))output=o;}
static void description(void*d,struct wl_output*o,const char*n){}
static const struct wl_output_listener listener={geometry,mode,done,scale,name,description};
static void global(void*d,struct wl_registry*r,uint32_t n,const char*i,uint32_t v){
 if(!strcmp(i,"zwlr_virtual_pointer_manager_v1"))manager=wl_registry_bind(r,n,&zwlr_virtual_pointer_manager_v1_interface,2);
 if(!strcmp(i,"wl_output")&&v>=4){struct wl_output*o=wl_registry_bind(r,n,&wl_output_interface,4);wl_output_add_listener(o,&listener,NULL);}
}
static void removed(void*d,struct wl_registry*r,uint32_t n){}
int main(int argc,char**argv){
 if(argc!=2)return 2;
 wanted=argv[1];struct wl_display*d=wl_display_connect(NULL);if(!d)return 3;
 struct wl_registry*r=wl_display_get_registry(d);struct wl_registry_listener l={global,removed};wl_registry_add_listener(r,&l,NULL);
 wl_display_roundtrip(d);wl_display_roundtrip(d);if(!manager||!output)return 4;
 struct zwlr_virtual_pointer_v1*p=zwlr_virtual_pointer_manager_v1_create_virtual_pointer_with_output(manager,NULL,output);
 char command;int a,b;
 while(scanf(" %c %d %d",&command,&a,&b)==3){
  struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);uint32_t ms=t.tv_sec*1000+t.tv_nsec/1000000;
  if(command=='M')zwlr_virtual_pointer_v1_motion_absolute(p,ms,a,b,1200,720);
  else if(command=='B')zwlr_virtual_pointer_v1_button(p,ms,a,b);
  else if(command=='S'){
   zwlr_virtual_pointer_v1_axis_source(p,WL_POINTER_AXIS_SOURCE_FINGER);
   if(b)zwlr_virtual_pointer_v1_axis(p,ms,a,wl_fixed_from_int(b));
   else zwlr_virtual_pointer_v1_axis_stop(p,ms,a);
  }
  else return 5;
  zwlr_virtual_pointer_v1_frame(p);if(wl_display_roundtrip(d)<0)return 6;puts("ok");fflush(stdout);
 }
 zwlr_virtual_pointer_v1_destroy(p);wl_display_flush(d);wl_display_disconnect(d);return 0;
}
