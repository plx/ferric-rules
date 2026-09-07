;; fact-slot-value returns every element of a multislot.
;; Level: basic
;; Covers: queries, fact-slot-value-multislot
(deftemplate sample (multislot values))
(deffacts seed (sample (values a b c)))
(defrule probe ?f <- (sample (values $?values)) => (printout t (fact-slot-value ?f values) crlf))
