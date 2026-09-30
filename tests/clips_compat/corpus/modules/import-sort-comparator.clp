;; An imported deffunction can be named as a sort predicate.
;; Level: interaction
;; Covers: modules, sort, import-function-named
(defmodule M (export deffunction exchange))
(deffunction exchange (?a ?b) (> ?a ?b))
(defmodule MAIN (import M deffunction exchange))
(defrule probe => (printout t (sort exchange (create$ 3 1 2)) crlf))
