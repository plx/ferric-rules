; Exists CE fires once regardless of how many matching facts
(deffacts startup
    (signal a)
    (signal b)
    (signal c))

(defrule any-signal
    (exists (signal ?))
    =>
    (printout t "signal detected" crlf))
