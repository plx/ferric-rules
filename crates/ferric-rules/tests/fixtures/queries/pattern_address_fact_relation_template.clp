;; fact-relation returns the unqualified template name.
;; Level: basic
;; Covers: queries, fact-relation-template
(deftemplate sample (slot value))
(deffacts seed (sample (value a)))
(defrule probe ?f <- (sample (value ?value)) => (printout t (fact-relation ?f) crlf))
