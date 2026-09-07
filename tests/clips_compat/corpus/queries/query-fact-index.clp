;; fact-index on a query-bound address returns the assertion index.
;; Level: interaction
;; Covers: queries, query-fact-index
(deftemplate item (slot value))
(deffacts seed (item (value 10)))
(defrule probe =>
    (do-for-fact ((?f item)) TRUE (printout t (fact-index ?f) crlf)))
