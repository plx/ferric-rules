(defmodule MAIN (export ?ALL))
(deftemplate item (slot id (type INTEGER)) (slot state (type SYMBOL)))
(defglobal ?*seen* = 0)
(deffacts items
  (item (id 1) (state ready))
  (item (id 2) (state ready))
  (item (id 3) (state ready))
  (blocked 2))
(defmodule WORK (import MAIN ?ALL))
(defrule consume
  ?item <- (item (id ?id) (state ready))
  (not (blocked ?id))
  =>
  (modify ?item (state done))
  (bind ?*seen* (+ ?*seen* 1))
  (printout t "done " ?id crlf))
