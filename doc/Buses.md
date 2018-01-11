Buses in the Program
====================

The testing infrastructure has two buses.  One is a many-to-one, and the other is one-to-many.


Control Channel
---------------

The control channel is a many-to-one interface.  Every Interface, Trigger, or other input mechanism will post messages to the Control channel.  These will be fed to the main testing infrastructure where they will be acted upon or dispatched as necessary.


Broadcast Channel
-----------------

Most data comes across the broadcast channel.  Broadcast data includes log messages, interface messages, and test result messages.