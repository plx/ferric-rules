(defmodule A (export deftemplate sample))
(defglobal ?*g* = A-global)
(deffunction owner () A-function)
(deffunction global-owner () ?*g*)
(deftemplate sample
 (slot owner (default-dynamic (owner)))
 (slot direct (default-dynamic ?*g*))
 (slot wrapped (default-dynamic (global-owner))))
(defmodule MAIN (import A deftemplate sample))
(defglobal ?*g* = MAIN-global)
(deffunction owner () MAIN-function)
(defrule run =>
 (bind ?f (assert (sample)))
 (printout t (fact-slot-value ?f owner) ":" (fact-slot-value ?f direct) ":" (fact-slot-value ?f wrapped) crlf))
