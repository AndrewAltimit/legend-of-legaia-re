# @category Legaia
# @runtime Jython
#
# Lists every program in the Ghidra project. Used to discover which overlays
# have been imported.

from ghidra.framework.model import DomainFile

project = state.getProject()
data = project.getProjectData()


def walk(folder, depth=0):
    for f in folder.getFiles():
        print("{}{}  [{}]".format("  " * depth, f.getName(), f.getContentType()))
    for sub in folder.getFolders():
        print("{}/{}/".format("  " * depth, sub.getName()))
        walk(sub, depth + 1)


walk(data.getRootFolder())
