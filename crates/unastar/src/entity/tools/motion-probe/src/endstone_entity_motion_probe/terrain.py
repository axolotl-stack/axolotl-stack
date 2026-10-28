"""Terrain postconditions for the disposable native controls, without SDK imports."""


def fill_verified(minimum, maximum, block, dispatch, block_type_at):
    """Return the number of native voxels verified, not a command success count.

    Reads every voxel, including the maximum corner, before actors can spawn.
    The caller retains the verified volume with the authored control evidence.
    """
    command = 'fill {} {} {} {} {} {} {}'.format(*minimum, *maximum, block)
    # Native fill reports false for zero changed blocks. Only the complete
    # requested postcondition proves preparation, including that no-op case.
    dispatch(command)
    verified = 0
    for x in range(minimum[0], maximum[0] + 1):
        for y in range(minimum[1], maximum[1] + 1):
            for z in range(minimum[2], maximum[2] + 1):
                actual = block_type_at(x, y, z)
                if actual != block:
                    raise RuntimeError(
                        f'terrain differs at {(x, y, z)}: expected {block}, observed {actual}')
                verified += 1
    return verified
