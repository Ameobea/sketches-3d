bl_info = {
    "name": "Quick gltf Export",
    "author": "Casey Primozic",
    "version": (1, 0),
    "blender": (2, 80, 0),
    "location": "Shortcut or Scene panel",
    "description": "Quick export to ~/dream/static/{filename}.glb",
    "category": "Import-Export",
}

import bpy
import os


class QuickExportGLTFOperator(bpy.types.Operator):
    bl_idname = "export_scene.quick_gltf"
    bl_label = "Quick Export glTF"
    bl_description = "Export current file to ~/dream/static/{filename}.glb"

    def execute(self, _ctx):
        blend_path = bpy.data.filepath
        if not blend_path:
            self.report({"WARNING"}, "Please save the .blend file first.")
            return {"CANCELLED"}

        blend_name = os.path.basename(blend_path)
        name_no_ext = os.path.splitext(blend_name)[0]

        export_path = os.path.join(f"/home/casey/dream/static/{name_no_ext}.glb")
        export_path = os.path.normpath(export_path)

        bpy.ops.export_scene.gltf(
            filepath=export_path,
            export_format="GLB",
            export_apply=True,
        )

        self.report({"INFO"}, f"Exported to {export_path}")
        return {"FINISHED"}


def draw_export_button(self, _ctx):
    layout = self.layout
    layout.operator("export_scene.quick_gltf", icon="EXPORT")


def register():
    bpy.utils.register_class(QuickExportGLTFOperator)
    bpy.types.SCENE_PT_scene.append(draw_export_button)


def unregister():
    bpy.types.SCENE_PT_scene.remove(draw_export_button)
    bpy.utils.unregister_class(QuickExportGLTFOperator)


if __name__ == "__main__":
    register()
