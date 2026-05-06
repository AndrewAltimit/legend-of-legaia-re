# @category Legaia
# @runtime Jython
#
# Print every program name in the project as `name [size]`. Used to
# discover which overlay program names exist for -process arguments.

project = state.getProject()
data = project.getProjectData()


def walk(folder, prefix=""):
    for f in folder.getFiles():
        print(prefix + f.getName())
    for sub in folder.getFolders():
        walk(sub, prefix + sub.getName() + "/")


walk(data.getRootFolder())
