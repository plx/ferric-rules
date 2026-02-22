; Rule with multiple patterns matching different fact types joined by variable
(deffacts startup
    (person Alice)
    (age Alice 30))

(defrule greet-with-age
    (person ?name)
    (age ?name ?a)
    =>
    (printout t ?name " is " ?a crlf))
