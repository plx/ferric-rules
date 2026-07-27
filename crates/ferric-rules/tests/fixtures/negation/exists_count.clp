; Exists fires exactly once regardless of how many facts match
(deffacts startup
    (signal a)
    (signal b)
    (signal c)
    (ready))

(defrule any-signal
    (ready)
    (exists (signal ?))
    =>
    (printout t "signal present" crlf))
