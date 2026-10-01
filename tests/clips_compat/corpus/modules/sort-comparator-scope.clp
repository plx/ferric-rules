;; A sort predicate name resolves in the module that calls sort.
;; Level: interaction
;; Covers: <, >, create$, deffunction, defmodule, export, import, sort
;; Run with load, reset, and run in a fresh environment.

(defmodule M (export deffunction run-sort))

(deffunction exchange (?a ?b) (> ?a ?b))

(deffunction run-sort (?items) (sort exchange ?items))

(defmodule MAIN (import M deffunction run-sort))

(deffunction exchange (?a ?b) (< ?a ?b))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (sort exchange (create$ 3 1 2)) " " (run-sort (create$ 3 1 2)) crlf))
