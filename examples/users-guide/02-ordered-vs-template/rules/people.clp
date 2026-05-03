(deftemplate person
    (slot name)
    (slot age (default 0))
    (multislot hobbies))

(defrule adult
    (person (name ?n) (age ?a))
    (test (>= ?a 18))
    =>
    (printout t ?n " is an adult" crlf))
