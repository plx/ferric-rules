;; Evaluating a missing slot must fail; skipped accesses have their own fixture.
(deftemplate item (slot value))
(deffacts seed (item (value 10)))
(defrule probe =>
  (do-for-all-facts ((?f item)) ?f:missing
    (printout t "unexpected" crlf)))
