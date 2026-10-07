(defmodule A (export deffunction pair))
(deffunction pair (?a ?b) (str-cat ?a ":" ?b))
(defmodule MAIN (import A deffunction pair))
(deffunction tail (?head $?rest) (create$ (length$ ?head) (length$ ?rest)))
(defgeneric total)
(defmethod total ((?a INTEGER) (?b INTEGER)) (+ ?a ?b))
(defrule run =>
 (printout t (A::pair (expand$ (create$ a b))) ":"
   (funcall pair (expand$ (create$ c d))) ":"
   (total (expand$ (create$ 1 2))) ":"
   (funcall + (expand$ (create$ 3 4))) ":"
   (tail (create$ a b) (expand$ (create$ 1 2))) crlf))
