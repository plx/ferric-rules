(defmodule A)
(defmodule B)
(defmodule MAIN)
(defrule run =>
  (printout t "result:["
    (focus (if TRUE then (printout t A) A) (if TRUE then (printout t B) B))
    "] stack:" (get-focus-stack) crlf))
