# @category Legaia
# @runtime Jython
#
# List every program in the open project. Run with the recursive flag so
# we see overlays too. Use to confirm which programs exist before running
# a multi-program sweep.

from ghidra.framework.model import DomainFolder

project = state.getProject()
root = project.getProjectData().getRootFolder()

def walk(folder):
    for f in folder.getFiles():
        print("FILE: {}  type={}".format(f.getPathname(), f.getContentType()))
    for sub in folder.getFolders():
        walk(sub)

walk(root)
print("done")
