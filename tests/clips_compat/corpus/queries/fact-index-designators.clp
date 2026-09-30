;; The fact functions and retract accept an integer fact index as well as a
;; fact address, and fact-index round-trips through them.
;; Level: interaction
;; Covers: queries, fact-index, fact-relation, fact-existp, fact-slot-value, fact-slot-names, retract
(deftemplate a (slot v))
(deftemplate b (slot v))
(deffacts seed (a (v 1)) (b (v 2)) (c 3))
(defrule probe
  ?f <- (b (v ?))
  =>
  (printout t (fact-index ?f) " " (fact-relation (fact-index ?f)) " " (fact-existp (+ 1 (fact-index ?f))) crlf)
  (printout t (fact-relation 1) " " (fact-relation 3) " " (fact-relation 0) " " (fact-relation 99) crlf)
  (printout t (fact-existp 0) " " (fact-existp 3) " " (fact-existp 4) crlf)
  (printout t (fact-slot-value 1 v) " " (fact-slot-value 3 implied) crlf)
  (printout t (fact-slot-names 2) crlf)
  (retract 3)
  (printout t (fact-existp 3) crlf)
  (bind ?i (fact-index ?f))
  (retract ?i)
  (printout t (fact-existp ?f) " " (fact-existp 2) crlf)
  (do-for-all-facts ((?x a)) TRUE
    (printout t (fact-index ?x) " " (fact-relation ?x) " " ?x:v crlf)))
