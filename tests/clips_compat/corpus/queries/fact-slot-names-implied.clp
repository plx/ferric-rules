;; An ordered fact has the single implied slot.
;; Level: basic
;; Covers: queries, fact-slot-names-implied
(deffacts seed (sample a))
(defrule probe ?f <- (sample a) => (printout t (fact-slot-names ?f) crlf))
