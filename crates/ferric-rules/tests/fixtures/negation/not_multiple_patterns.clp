; Not with variable binding: only persons not in the banned list are allowed
(deffacts startup
    (person Alice)
    (person Bob)
    (banned Bob))

(defrule allowed
    (person ?name)
    (not (banned ?name))
    =>
    (printout t ?name " allowed" crlf))
