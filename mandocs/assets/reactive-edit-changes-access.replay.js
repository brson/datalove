// Recorded by crates/datalove-doc-fixtures from replays/reactive-edit-changes-access.world.
// Do not edit: run `BLESS=1 cargo test -p datalove-doc-fixtures`.
DataloveReplay.register("reactive-edit-changes-access", {
  "version": 1,
  "steps": [
    {
      "input": "var b = 125",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Slot": {
                "name": "b",
                "ty": "int",
                "value": "125"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "var b = 125",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "b",
                  "ty": "int",
                  "value": "125"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [
              [
                "b",
                "125"
              ]
            ],
            "provides": [
              "b"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "b",
          "ty": "int",
          "value": "125"
        }
      ]
    },
    {
      "input": "set b = b * 2",
      "outcome": {
        "Unit": {
          "Ran": []
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "var b = 125",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "b",
                  "ty": "int",
                  "value": "125"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [
              [
                "b",
                "250"
              ]
            ],
            "provides": [
              "b"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 2,
          "expression": false,
          "source": "set b = b * 2",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [],
            "provides": [],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [
              "b"
            ],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "b",
          "ty": "int",
          "value": "250"
        }
      ]
    },
    {
      "input": "set b = b + 1",
      "outcome": {
        "Unit": {
          "Ran": []
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "var b = 125",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "b",
                  "ty": "int",
                  "value": "125"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [
              [
                "b",
                "251"
              ]
            ],
            "provides": [
              "b"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 2,
          "expression": false,
          "source": "set b = b * 2",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [],
            "provides": [],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [
              "b"
            ],
            "revives": []
          }
        },
        {
          "number": 3,
          "expression": false,
          "source": "set b = b + 1",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 3,
            "diagnostics": "",
            "bindings": [],
            "provides": [],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [
              "b"
            ],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "b",
          "ty": "int",
          "value": "251"
        }
      ]
    },
    {
      "input": "/edit 2 let c = b + 1",
      "outcome": {
        "Edited": [
          {
            "unit": 1,
            "compiled": false,
            "eval": {
              "Ran": [
                {
                  "Slot": {
                    "name": "b",
                    "ty": "int",
                    "value": "125"
                  }
                }
              ]
            }
          },
          {
            "unit": 2,
            "compiled": true,
            "eval": {
              "Ran": [
                {
                  "Value": {
                    "name": "c",
                    "ty": "int",
                    "value": "126"
                  }
                }
              ]
            }
          },
          {
            "unit": 3,
            "compiled": false,
            "eval": {
              "Ran": []
            }
          }
        ]
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "var b = 125",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "b",
                  "ty": "int",
                  "value": "125"
                }
              }
            ]
          },
          "compiled": false,
          "ran": true,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [
              [
                "b",
                "126"
              ]
            ],
            "provides": [
              "b"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 2,
          "expression": false,
          "source": "let c = b + 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "c",
                  "ty": "int",
                  "value": "126"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [
              [
                "c",
                "126"
              ]
            ],
            "provides": [
              "c"
            ],
            "reads": [
              "b"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 3,
          "expression": false,
          "source": "set b = b + 1",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": true,
          "detail": {
            "unit": 3,
            "diagnostics": "",
            "bindings": [],
            "provides": [],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [
              "b"
            ],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "b",
          "ty": "int",
          "value": "126"
        },
        {
          "name": "c",
          "ty": "int",
          "value": "126"
        }
      ]
    }
  ]
});
