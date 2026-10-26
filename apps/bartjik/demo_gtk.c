/* Loaded only into the disposable synthetic Waybar, never the session bar. */
#include <gtk/gtk.h>
#include <stdio.h>
static FILE *out;
static void visit(GtkWidget*w,gpointer root){
 const char*n=gtk_widget_get_name(w);int x=0,y=0;
 gtk_widget_translate_coordinates(w,root,0,0,&x,&y);
 if(g_str_has_prefix(n,"custom-") && !g_object_get_data(G_OBJECT(w),"demo-tooltip")){
   GKeyFile*data=g_key_file_new();const char*path=g_getenv("BARTJIK_GTK_TOOLTIPS");
   if(path&&g_key_file_load_from_file(data,path,G_KEY_FILE_NONE,NULL)){
     gchar*tip=g_key_file_get_string(data,n,"text",NULL);
     if(tip){gtk_widget_set_tooltip_text(w,tip);g_free(tip);}
   }
   g_key_file_free(data);g_object_set_data(G_OBJECT(w),"demo-tooltip",GINT_TO_POINTER(1));
 }
 if(g_str_has_prefix(n,"custom-") || !strcmp(n,"ai") || !strcmp(n,"tray"))
   fprintf(out,"%s %d %d %d %d\n",n,x,y,gtk_widget_get_allocated_width(w),gtk_widget_get_allocated_height(w));
 if(!strcmp(n,"tray") && GTK_IS_CONTAINER(w)){
   GList*children=gtk_container_get_children(GTK_CONTAINER(w));int index=0;
   for(GList*i=children;i;i=i->next){GtkWidget*child=i->data;int cx=0,cy=0;
     gtk_widget_translate_coordinates(child,root,0,0,&cx,&cy);
     fprintf(out,"tray-%d %d %d %d %d\n",index++,cx,cy,gtk_widget_get_allocated_width(child),gtk_widget_get_allocated_height(child));
     gchar*tip=gtk_widget_get_tooltip_markup(child);
     const char*ids[]={"TelegramDesktop","ZapZap","vlc"};
     const char*titles[]={"Telegram","WhatsApp","VLC"};
     for(int j=0;tip&&j<3;j++)if(strstr(tip,titles[j]))
       fprintf(out,"tray-%s %d %d %d %d\n",ids[j],cx,cy,gtk_widget_get_allocated_width(child),gtk_widget_get_allocated_height(child));
     g_free(tip);
   }
   g_list_free(children);
 }
 if(GTK_IS_CONTAINER(w))gtk_container_foreach(GTK_CONTAINER(w),visit,root);
}
static gboolean snapshot(gpointer unused){
 const char*p=g_getenv("BARTJIK_GTK_GEOMETRY");if(!p)return TRUE;
 gchar*tmp=g_strconcat(p,".tmp",NULL);out=fopen(tmp,"w");if(!out){g_free(tmp);return TRUE;}
 GList*ws=gtk_window_list_toplevels();for(GList*i=ws;i;i=i->next){GtkWidget*w=i->data;if(!gtk_widget_get_visible(w))continue;
 fprintf(out,"window %s %d %d\n",gtk_widget_get_name(w),gtk_widget_get_allocated_width(w),gtk_widget_get_allocated_height(w));
 GdkWindow*gw=gtk_widget_get_window(w);if(gw){int px,py;GdkModifierType mods;gdk_window_get_device_position(gw,gdk_seat_get_pointer(gdk_display_get_default_seat(gdk_display_get_default())),&px,&py,&mods);fprintf(out,"pointer %d %d\n",px,py);}
 visit(w,w);}
 g_list_free(ws);fclose(out);rename(tmp,p);g_free(tmp);return TRUE;
}
G_MODULE_EXPORT void gtk_module_init(gint*argc,gchar***argv){g_timeout_add(80,snapshot,NULL);}
