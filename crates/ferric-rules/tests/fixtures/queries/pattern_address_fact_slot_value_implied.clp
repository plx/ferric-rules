;; Ordered fact fields are exposed as the implied multislot.
;; Level: basic
;; Covers: queries, fact-slot-value-implied
(deffacts seed (sample a b c))
(defrule probe ?f <- (sample $?values) => (printout t (fact-slot-value ?f implied) crlf))
