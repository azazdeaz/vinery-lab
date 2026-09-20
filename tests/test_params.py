"""The params classes as Python sees them: keyword construction, and a stub that
matches the compiled module.

Needs only the extension, not Isaac Lab, so it runs everywhere `import vinerylab`
does.
"""

from __future__ import annotations

import ast
import pathlib

import pytest

import vinerylab

STUB = pathlib.Path(vinerylab.__file__).with_name("_core.pyi")


def params_attrs(params_cls: type) -> set[str]:
    """The settable fields of a `*Params` pyclass.

    PyO3 exposes `get_all`/`set_all` fields as class-level descriptors and has
    no `__dict__` to read them from, so they are recovered off the class with
    the dunders and methods filtered out.
    """
    return {
        name
        for name in dir(params_cls)
        if not name.startswith("_") and not callable(getattr(params_cls, name, None))
    }


def stub_classes() -> dict[str, set[str]]:
    """Every class the stub declares, with its annotated attributes."""
    classes = {}
    for node in ast.parse(STUB.read_text()).body:
        if isinstance(node, ast.ClassDef):
            classes[node.name] = {
                item.target.id
                for item in node.body
                if isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name)
            }
    return classes


def test_a_fragment_takes_its_fields_as_keywords():
    pole = vinerylab.PoleParams(radius=0.05, sides=12)
    assert pole.radius == pytest.approx(0.05)
    assert pole.sides == 12
    assert vinerylab.PoleParams().sides == 8, "an omitted field keeps its default"
    assert "sides: 12" in repr(pole)


@pytest.mark.parametrize(
    "kwargs",
    [{"radious": 0.05}, {"sides": 1.5}, {"radius": "thick"}],
    ids=["misspelt name", "float for an int", "text for a float"],
)
def test_a_wrong_keyword_or_type_is_a_type_error(kwargs):
    with pytest.raises(TypeError):
        vinerylab.PoleParams(**kwargs)


def test_the_stub_declares_what_the_module_exports():
    """The generated stub and the compiled module agree, class for class and
    field for field. A stale extension, or a class the module forgot to
    register, shows up here rather than as a silent hole in the IDE."""
    stub = stub_classes()
    exported = {name for name in vinerylab.__all__ if name[0].isupper()}
    assert set(stub) == exported
    for name in exported - {"VineyardParams"}:
        assert stub[name] == params_attrs(getattr(vinerylab, name)), name
    assert stub["VineyardParams"] == params_attrs(vinerylab.VineyardParams)


def test_the_aggregate_holds_live_fragments():
    params = vinerylab.VineyardParams(pole=vinerylab.PoleParams(sides=5))
    assert params.pole.sides == 5
    params.terrain.detail = 8
    assert params.terrain.detail == 8, "an attribute edit lands on the shared object"
