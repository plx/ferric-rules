;; Parsing an eval expression declares the implied template of an assertion
;; it contains, even though the assertion never runs.
(defrule probe =>
  (printout t (get-deftemplate-list) crlf)
  (eval "(if FALSE then (assert (hidden)))")
  (printout t (get-deftemplate-list) " " (deftemplate-slot-names hidden) crlf))
