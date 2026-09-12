;; Exported generic dispatch works from an importing module.
;; Level: interaction
;; Covers: generics, import-generic
(defmodule UTIL (export defgeneric classify))
(defgeneric classify)
(defmethod classify ((?x INTEGER)) integer)
(defmodule MAIN (import UTIL defgeneric classify))
(defrule probe => (printout t (classify 4) crlf))
