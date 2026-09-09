;; fact-relation returns the name of an ordered fact.
;; Level: basic
;; Covers: queries, fact-relation-ordered
(deffacts seed (sample a))
(defrule probe ?f <- (sample a) => (printout t (fact-relation ?f) crlf))
