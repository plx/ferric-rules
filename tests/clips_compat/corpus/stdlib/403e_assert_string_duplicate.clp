(deftemplate p (slot x))
(defrule run =>
  (bind ?f (assert-string "(p (x 7))"))
  (printout t (fact-existp ?f) ":" (assert-string "(p (x 7))") ":"
    (fact-slot-value ?f x) crlf))
