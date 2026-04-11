;; Template with multiple slot types for testing.
(deftemplate person
  (slot name)
  (slot age)
  (slot active))

(defrule greet-person
  (person (name ?n) (age ?a) (active TRUE))
  =>
  (printout t "Hello " ?n " age " ?a crlf))
