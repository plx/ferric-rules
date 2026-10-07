(defmodule OTHER (export deffunction ?ALL))
(deffunction helper (?x) ?x)
(defmodule MAIN)
(deffunction caller (?x) (helper ?x))
