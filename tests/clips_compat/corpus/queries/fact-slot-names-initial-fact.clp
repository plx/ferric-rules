;; initial-fact has no slots, whether named by its fact index or its address;
;; an ordered fact has the one slot implied.
;; Level: boundary
;; Covers: queries, fact-slot-names, fact-index, initial-fact
(deffacts seed (point 1 2))
(defrule probe
  ?i <- (initial-fact)
  ?p <- (point $?)
  =>
  (printout t (fact-index ?i) " " (fact-slot-names 0) " " (fact-slot-names ?i) crlf)
  (printout t (fact-slot-names ?p) " " (fact-relation 0) crlf))
