(defmodule MAIN (export ?ALL))
(defglobal MAIN ?*proximity* = 9)
(defmodule SCORE (import MAIN ?ALL))
(defrule SCORE::should-be-ok
   (attempt1)
   (test (<= 3 ?*proximity*))
   =>)
