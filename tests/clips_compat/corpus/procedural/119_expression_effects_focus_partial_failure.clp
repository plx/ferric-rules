(defmodule A)
(defmodule MAIN)
(defrule run =>
  (printout t "result:["
    (focus (if TRUE then (printout t M) MISSING) (if TRUE then (printout t A) A))
    "] stack:" (get-focus-stack) crlf))
